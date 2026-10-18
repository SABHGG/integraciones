//! Máquina de ejecución de la cadena lineal de bloques.
//!
//! Recorre las instancias de `bridge_versions.definition` en orden, resuelve el
//! input y la config de cada bloque contra el contexto acumulado, valida la
//! entrada contra el `inputSchema` del bloque, invoca el motor de bloques y
//! registra la salida **nombrada** en el contexto. El primer error termina la
//! ejecución (fail-fast) y queda registrado en el trace con el bloque culpable.

use std::collections::HashSet;
use std::time::Instant;

use domain::definition::{BlockInstance, BridgeDefinition};
use domain::execution::{ErrorCode, ErrorPayload, ExecutionStatus, StepStatus};
use domain::refs;
use domain::schema::{self, SchemaError};
use serde_json::{Map, Value as Json, json};

use crate::invoker::{BlockInvoker, BuiltInRequest, InvokeRequest};

/// Timeout por defecto de un bloque cuando la instancia no lo define (ms).
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// Raíces reservadas del contexto de ejecución (design.md → "B puro").
pub const RESERVED_ROOTS: [&str; 2] = ["trigger", "now"];

/// Entrada del ejecutor: la definición de la versión, el trigger y el instante
/// de inicio (`$.now`).
pub struct ChainRun<'a> {
    /// Definición snapshot con la que correr.
    pub definition: &'a BridgeDefinition,
    /// Datos del trigger ya normalizados según su tipo (`$.trigger`).
    pub trigger: Json,
    /// Timestamp ISO de inicio (`$.now`).
    pub now: String,
}

impl<'a> ChainRun<'a> {
    /// Construye la corrida tomando `$.now` del reloj del sistema.
    pub fn new(definition: &'a BridgeDefinition, trigger: Json) -> Self {
        Self {
            definition,
            trigger,
            now: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Un paso del trace: el IO completo de un bloque en secuencia.
#[derive(Debug, Clone)]
pub struct ExecutionStep {
    /// Posición en la cadena (1-based; `execution_steps.seq`).
    pub seq: i32,
    /// Id de la instancia dentro de la definición.
    pub block_instance_id: String,
    /// Nombre del bloque (el que usa el contexto para su salida).
    pub block_name: String,
    /// Estado del paso.
    pub status: StepStatus,
    /// Entrada del bloque: la resuelta, o el mapeo crudo si falló al resolver.
    pub input: Json,
    /// Salida del bloque, si terminó.
    pub output: Option<Json>,
    /// Error del paso, si falló.
    pub error: Option<ErrorPayload>,
    /// Duración en ms. `None` cuando el sandbox no llegó a invocarse.
    pub duration_ms: Option<i32>,
}

/// Resultado de una ejecución: estado, salida final, error y trace.
#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    /// Estado final de la ejecución.
    pub status: ExecutionStatus,
    /// Salida del último bloque ejecutado (el `data` del sobre de invocación).
    pub output: Option<Json>,
    /// Error estructurado del primer fallo, si lo hubo.
    pub error: Option<ErrorPayload>,
    /// Contexto acumulado final (`{trigger, now, <bloque>: <salida>}`).
    pub context: Json,
    /// Pasos registrados hasta el fallo (o hasta el final).
    pub steps: Vec<ExecutionStep>,
}

impl ExecutionOutcome {
    /// `true` si la ejecución terminó sin error.
    pub fn is_success(&self) -> bool {
        self.status == ExecutionStatus::Succeeded
    }
}

/// Ejecutor de cadenas lineales sobre un [`BlockInvoker`].
pub struct Executor<I> {
    invoker: I,
    default_timeout_ms: u64,
}

impl<I: BlockInvoker> Executor<I> {
    /// Crea el ejecutor con el timeout por defecto del puente.
    pub fn new(invoker: I) -> Self {
        Self {
            invoker,
            default_timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }

    /// Override del timeout por defecto (los bloques pueden overridearlo).
    pub fn with_default_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.default_timeout_ms = timeout_ms;
        self
    }

    /// Invoker en uso (útil para inspeccionar los tests).
    pub fn invoker(&self) -> &I {
        &self.invoker
    }

    /// Ejecuta la cadena completa.
    pub async fn run(&self, run: ChainRun<'_>) -> ExecutionOutcome {
        let mut context = initial_context(run.trigger, run.now);

        // Invariantes que el ejecutor no puede tolerar (la publicación ya las
        // rechaza en la tarea 4.2; aquí se falla cerrado por si el snapshot
        // llega manipulado).
        if let Some(error) = definition_error(run.definition) {
            return ExecutionOutcome {
                status: ExecutionStatus::Failed,
                output: None,
                error: Some(error),
                context,
                steps: Vec::new(),
            };
        }

        let mut steps = Vec::with_capacity(run.definition.blocks.len());
        let mut last_output: Option<Json> = None;

        for (index, block) in run.definition.blocks.iter().enumerate() {
            let seq = index as i32 + 1;

            // La config se resuelve aunque el sandbox de CUSTOM_LOGIC no la
            // consuma: una ref rota en la config debe fallar, no pasar muda.
            let resolved_config = match refs::resolve_config(&context, &block.config) {
                Ok(value) => value,
                Err(error) => {
                    let step = failed_step(
                        seq,
                        block,
                        block.config.clone(),
                        ref_error(block, error),
                        None,
                    );
                    return failed_outcome(steps, step, context);
                }
            };

            let resolved_input = match refs::resolve_input(&context, &block.input) {
                Ok(value) => value,
                Err(error) => {
                    let step = failed_step(
                        seq,
                        block,
                        block.input.clone(),
                        ref_error(block, error),
                        None,
                    );
                    return failed_outcome(steps, step, context);
                }
            };

            if let Err(error) = schema::validate(&block.input_schema, &resolved_input) {
                let step = failed_step(
                    seq,
                    block,
                    resolved_input,
                    validation_error(block, error),
                    None,
                );
                return failed_outcome(steps, step, context);
            }

            let timeout_ms = block.timeout_ms.unwrap_or(self.default_timeout_ms);
            let started = Instant::now();
            // CUSTOM_LOGIC (con code) → sandbox; sin code → bloque built-in
            // `http` de la biblioteca base: el host ejecuta desde la config.
            let result = if let Some(code) = block.code.as_deref() {
                self.invoker
                    .invoke(InvokeRequest {
                        code,
                        input: resolved_input.clone(),
                        config: resolved_config,
                        timeout_ms,
                    })
                    .await
            } else {
                self.invoker
                    .invoke_built_in_http(BuiltInRequest {
                        config: &resolved_config,
                        timeout_ms,
                    })
                    .await
            };
            let duration_ms = Some(elapsed_ms(started));

            match result {
                Ok(output) => {
                    if let Json::Object(map) = &mut context {
                        map.insert(block.name.clone(), output.clone());
                    }
                    steps.push(ExecutionStep {
                        seq,
                        block_instance_id: block.id.clone(),
                        block_name: block.name.clone(),
                        status: StepStatus::Succeeded,
                        input: resolved_input,
                        output: Some(output.clone()),
                        error: None,
                        duration_ms,
                    });
                    last_output = Some(output);
                }
                Err(error) => {
                    let step = failed_step(
                        seq,
                        block,
                        resolved_input,
                        sandbox_error(block, error),
                        duration_ms,
                    );
                    return failed_outcome(steps, step, context);
                }
            }
        }

        ExecutionOutcome {
            status: ExecutionStatus::Succeeded,
            output: last_output,
            error: None,
            context,
            steps,
        }
    }
}

/// Construye el outcome fallido a partir del paso culpable.
fn failed_outcome(steps: Vec<ExecutionStep>, step: ExecutionStep, context: Json) -> ExecutionOutcome {
    let error = step
        .error
        .clone()
        .expect("un paso fallido siempre lleva error");
    let mut steps = steps;
    steps.push(step);
    ExecutionOutcome {
        status: ExecutionStatus::Failed,
        output: None,
        error: Some(error),
        context,
        steps,
    }
}

/// Construye un paso fallido.
fn failed_step(
    seq: i32,
    block: &BlockInstance,
    input: Json,
    error: ErrorPayload,
    duration_ms: Option<i32>,
) -> ExecutionStep {
    ExecutionStep {
        seq,
        block_instance_id: block.id.clone(),
        block_name: block.name.clone(),
        status: StepStatus::Failed,
        input,
        output: None,
        error: Some(error),
        duration_ms,
    }
}

/// Valida invariantes de la definición que el ejecutor no puede tolerar:
/// nombres reservados (colisionarían con `$.trigger` / `$.now`) y duplicados
/// (dos bloques con el mismo nombre se pisarían en el contexto).
fn definition_error(definition: &BridgeDefinition) -> Option<ErrorPayload> {
    let mut seen: HashSet<&str> = HashSet::new();
    for block in &definition.blocks {
        if RESERVED_ROOTS.contains(&block.name.as_str()) {
            return Some(
                ErrorPayload::new(
                    ErrorCode::ValidationFailed,
                    format!(
                        "el bloque \"{}\" usa el nombre reservado \"{}\", que colisiona con la raíz ${}.{}",
                        block.name, block.name, "", block.name
                    ),
                )
                .with_block(block.name.clone())
                .with_details(json!({
                    "reason": "BLOCK_NAME_RESERVED",
                    "reserved": RESERVED_ROOTS,
                })),
            );
        }
        if !seen.insert(block.name.as_str()) {
            return Some(
                ErrorPayload::new(
                    ErrorCode::ValidationFailed,
                    format!(
                        "hay más de un bloque con el nombre \"{}\": sus salidas se pisarían en el contexto",
                        block.name
                    ),
                )
                .with_block(block.name.clone())
                .with_details(json!({ "reason": "DUPLICATE_BLOCK_NAME" })),
            );
        }
    }
    None
}

/// Traduce un fallo de resolución de referencias al contrato de error.
fn ref_error(block: &BlockInstance, error: refs::RefError) -> ErrorPayload {
    ErrorPayload::new(ErrorCode::RefUnresolved, error.to_string())
        .with_block(block.name.clone())
}

/// Traduce un fallo de validación de schema al contrato de error.
fn validation_error(block: &BlockInstance, error: SchemaError) -> ErrorPayload {
    match error {
        SchemaError::Invalid { errors } => ErrorPayload::new(
            ErrorCode::ValidationFailed,
            format!(
                "la entrada del bloque \"{}\" no cumple su inputSchema: {}",
                block.name,
                errors
                    .iter()
                    .map(|e| format!("{} ({})", e.message, e.path))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        )
        .with_block(block.name.clone())
        .with_details(json!({
            "reason": "SCHEMA_VIOLATION",
            "violations": errors
                .iter()
                .map(|e| json!({ "path": e.path, "message": e.message }))
                .collect::<Vec<_>>(),
        })),
        SchemaError::Unsupported { path, reason } => ErrorPayload::new(
            ErrorCode::ValidationFailed,
            format!(
                "el inputSchema del bloque \"{}\" no está soportado en {path}: {reason}",
                block.name
            ),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "UNSUPPORTED_SCHEMA", "path": path })),
    }
}

/// Traduce un fallo del sandbox al contrato de error del ejecutor.
fn sandbox_error(block: &BlockInstance, error: sandbox::SandboxError) -> ErrorPayload {
    use sandbox::SandboxError;

    match error {
        SandboxError::Timeout => ErrorPayload::new(
            ErrorCode::Timeout,
            format!("el bloque \"{}\" excedió el timeout", block.name),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "TIMEOUT" })),
        SandboxError::MemoryLimit => ErrorPayload::new(
            ErrorCode::BlockFailed,
            format!("el bloque \"{}\" agotó el límite de memoria", block.name),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "MEMORY_LIMIT" })),
        SandboxError::JsError { message } => ErrorPayload::new(
            ErrorCode::BlockFailed,
            format!("el bloque \"{}\" lanzó un error: {message}", block.name),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "JS_ERROR", "message": message })),
        SandboxError::HttpDenied { domain } => ErrorPayload::new(
            ErrorCode::BlockFailed,
            format!(
                "el bloque \"{}\" intentó salir al dominio no permitido \"{domain}\"",
                block.name
            ),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "HTTP_DENIED", "domain": domain })),
        SandboxError::HostError { message } => ErrorPayload::new(
            ErrorCode::BlockFailed,
            format!("el bloque \"{}\" falló en el host: {message}", block.name),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "HOST_ERROR", "message": message })),
    }
}

/// Duración en ms acotada al rango de `execution_steps.duration_ms`.
fn elapsed_ms(started: Instant) -> i32 {
    started.elapsed().as_millis().min(i32::MAX as u128) as i32
}

/// Construye el contexto inicial de una ejecución.
fn initial_context(trigger: Json, now: String) -> Json {
    let mut context = Map::new();
    context.insert("trigger".to_string(), trigger);
    context.insert("now".to_string(), Json::String(now));
    Json::Object(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::future::Future;

    use domain::definition::{TriggerDefinition, TriggerType};
    use sandbox::SandboxError;
    use serde_json::Value;

    /// Invoker falso: responde una cola prefijada y graba las peticiones.
    #[derive(Default)]
    struct MockInvoker {
        responses: RefCell<VecDeque<Result<Value, SandboxError>>>,
        builtins: RefCell<VecDeque<Result<Value, SandboxError>>>,
        calls: RefCell<Vec<(String, Value, u64)>>,
        builtin_calls: RefCell<Vec<(Value, u64)>>,
    }

    impl MockInvoker {
        fn new(responses: Vec<Result<Value, SandboxError>>) -> Self {
            Self {
                responses: RefCell::new(responses.into()),
                builtins: RefCell::new(VecDeque::new()),
                calls: RefCell::new(Vec::new()),
                builtin_calls: RefCell::new(Vec::new()),
            }
        }

        /// Mock preparado para bloques built-in http (sin code).
        fn con_builtin(builtins: Vec<Result<Value, SandboxError>>) -> Self {
            let mut mock = Self::new(vec![]);
            mock.builtins = RefCell::new(builtins.into());
            mock
        }

        fn calls(&self) -> usize {
            self.calls.borrow().len()
        }

        fn builtin_calls(&self) -> usize {
            self.builtin_calls.borrow().len()
        }

        fn call(&self, index: usize) -> (String, Value, u64) {
            self.calls.borrow()[index].clone()
        }
    }

    impl BlockInvoker for MockInvoker {
        fn invoke(
            &self,
            request: InvokeRequest<'_>,
        ) -> impl Future<Output = Result<Value, SandboxError>> {
            self.calls.borrow_mut().push((
                request.code.to_string(),
                request.input.clone(),
                request.timeout_ms,
            ));
            std::future::ready(
                self.responses
                    .borrow_mut()
                    .pop_front()
                    .expect("el ejecutor pidió más bloques de los previstos"),
            )
        }

        async fn invoke_built_in_http(
            &self,
            request: crate::invoker::BuiltInRequest<'_>,
        ) -> Result<Value, SandboxError> {
            self.builtin_calls
                .borrow_mut()
                .push((request.config.clone(), request.timeout_ms));
            self.builtins
                .borrow_mut()
                .pop_front()
                .expect("el ejecutor pidió más bloques built-in de los previstos")
        }
    }

    fn block(name: &str, code: &str, input: Value, input_schema: Value) -> BlockInstance {
        BlockInstance {
            id: format!("instancia-{name}"),
            name: name.to_string(),
            block_id: None,
            block_version_id: None,
            code: Some(code.to_string()),
            input,
            config: json!({}),
            input_schema,
            output_schema: json!({}),
            timeout_ms: None,
        }
    }

    fn definition(blocks: Vec<BlockInstance>) -> BridgeDefinition {
        BridgeDefinition {
            trigger: TriggerDefinition {
                trigger_type: TriggerType::Http,
                method: Some("POST".to_string()),
                path: Some("/x/demo".to_string()),
            },
            input_schema: None,
            blocks,
        }
    }

    fn run<'a>(definition: &'a BridgeDefinition) -> ChainRun<'a> {
        ChainRun {
            definition,
            trigger: json!({ "body": { "userId": "u-1", "cantidad": 2 }, "query": {} }),
            now: "2025-01-02T03:04:05Z".to_string(),
        }
    }

    fn ok(value: Value) -> Result<Value, SandboxError> {
        Ok(value)
    }

    // ── cadena feliz ───────────────────────────────────────────────────

    #[tokio::test]
    async fn cadena_lineal_resuelve_refs_invoca_y_registra_salidas_nombradas() {
        let definition = definition(vec![
            block(
                "usuario",
                "export default (input) => ({ id: input.userId })",
                json!({ "userId": "$.trigger.body.userId", "hora": "$.now" }),
                json!({ "type": "object", "required": ["userId"] }),
            ),
            block(
                "pedido",
                "export default (input) => ({ total: input.cantidad * 10 })",
                json!({ "cantidad": "$.trigger.body.cantidad", "id": "$.usuario.id" }),
                json!({ "type": "object", "required": ["cantidad", "id"] }),
            ),
        ]);
        let invoker = MockInvoker::new(vec![
            ok(json!({ "id": "u-1" })),
            ok(json!({ "total": 20 })),
        ]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(outcome.is_success(), "{:?}", outcome.error);
        assert_eq!(outcome.output, Some(json!({ "total": 20 })));
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.steps[0].seq, 1);
        assert_eq!(outcome.steps[0].block_name, "usuario");
        assert_eq!(outcome.steps[0].block_instance_id, "instancia-usuario");
        assert_eq!(outcome.steps[0].status, StepStatus::Succeeded);
        assert_eq!(
            outcome.steps[0].input,
            json!({ "userId": "u-1", "hora": "2025-01-02T03:04:05Z" })
        );
        assert!(outcome.steps[0].duration_ms.is_some());
        assert_eq!(outcome.steps[1].seq, 2);
        assert_eq!(
            outcome.steps[1].input,
            json!({ "cantidad": 2, "id": "u-1" })
        );

        // El segundo bloque recibe la salida nombrada del primero ya resuelta.
        assert_eq!(
            executor.invoker().call(1).1,
            json!({ "cantidad": 2, "id": "u-1" })
        );

        // Contexto acumulado final.
        assert_eq!(outcome.context["trigger"]["body"]["userId"], json!("u-1"));
        assert_eq!(outcome.context["now"], json!("2025-01-02T03:04:05Z"));
        assert_eq!(outcome.context["usuario"], json!({ "id": "u-1" }));
        assert_eq!(outcome.context["pedido"], json!({ "total": 20 }));
    }

    #[tokio::test]
    async fn el_contexto_inicial_tiene_solo_trigger_y_now() {
        let definition = definition(vec![]);
        let executor = Executor::new(MockInvoker::new(vec![]));
        let outcome = executor.run(run(&definition)).await;
        assert_eq!(outcome.context["trigger"]["body"]["cantidad"], json!(2));
        assert_eq!(outcome.context.as_object().unwrap().len(), 2);
    }

    // ── fail-fast ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn el_primer_error_detiene_la_cadena_y_marca_el_paso_culpable() {
        let definition = definition(vec![
            block("uno", "code-1", json!({}), json!({})),
            block("dos", "throw", json!({}), json!({})),
            block("tres", "code-3", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![
            ok(json!({ "paso": 1 })),
            Err(SandboxError::JsError {
                message: "boom".to_string(),
            }),
            ok(json!({ "nunca": true })),
        ]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(!outcome.is_success());
        assert_eq!(outcome.status, ExecutionStatus::Failed);
        assert_eq!(executor.invoker().calls(), 2, "el tercer bloque no se invoca");
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.steps[0].status, StepStatus::Succeeded);
        assert_eq!(outcome.steps[0].output, Some(json!({ "paso": 1 })));
        assert_eq!(outcome.steps[1].status, StepStatus::Failed);
        assert_eq!(outcome.steps[1].block_name, "dos");
        assert_eq!(outcome.steps[1].output, None);
        assert!(outcome.steps[1].duration_ms.is_some());
        assert!(outcome.context.get("tres").is_none());

        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::BlockFailed);
        assert_eq!(error.block.as_deref(), Some("dos"));
        assert!(error.message.contains("boom"), "{}", error.message);
        assert_eq!(error.details.unwrap()["reason"], json!("JS_ERROR"));
    }

    #[tokio::test]
    async fn timeout_del_sandbox_se_mapea_a_timeout_del_contrato() {
        let definition = definition(vec![block("lento", "while(true){}", json!({}), json!({}))]);
        let invoker = MockInvoker::new(vec![Err(SandboxError::Timeout)]);
        let outcome = Executor::new(invoker).run(run(&definition)).await;

        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::Timeout);
        assert_eq!(error.code.as_str(), "TIMEOUT");
        assert_eq!(error.block.as_deref(), Some("lento"));
        assert_eq!(outcome.steps[0].status, StepStatus::Failed);
    }

    #[tokio::test]
    async fn la_denegacion_de_dominio_se_reporta_como_bloque_fallido() {
        let definition = definition(vec![block("http", "code", json!({}), json!({}))]);
        let invoker = MockInvoker::new(vec![Err(SandboxError::HttpDenied {
            domain: "evil.example".to_string(),
        })]);
        let outcome = Executor::new(invoker).run(run(&definition)).await;

        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::BlockFailed);
        assert_eq!(error.details.unwrap()["domain"], json!("evil.example"));
    }

    // ── validación de entrada ──────────────────────────────────────────

    #[tokio::test]
    async fn violacion_de_input_schema_falla_antes_de_invocar_el_sandbox() {
        let definition = definition(vec![
            block(
                "origen",
                "code-1",
                json!({}),
                json!({}),
            ),
            block(
                "destino",
                "code-2",
                json!({ "id": "$.origen" }),
                json!({
                    "type": "object",
                    "required": ["id"],
                    "properties": { "id": { "type": "string" } }
                }),
            ),
            block("tercero", "code-3", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![ok(json!(42)), ok(json!({})), ok(json!({}))]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(!outcome.is_success());
        assert_eq!(
            executor.invoker().calls(),
            1,
            "el bloque inválido no llega al sandbox"
        );
        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::ValidationFailed);
        assert_eq!(error.block.as_deref(), Some("destino"));
        assert!(error.message.contains("id"), "{}", error.message);
        let details = error.details.expect("detalles");
        assert_eq!(details["reason"], json!("SCHEMA_VIOLATION"));
        assert_eq!(details["violations"][0]["path"], json!("$.id"));

        // El paso culpable registra la entrada resuelta y no una duración.
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.steps[1].input, json!({ "id": 42 }));
        assert_eq!(outcome.steps[1].duration_ms, None);
    }

    #[tokio::test]
    async fn input_schema_no_soportado_falla_cerrado_y_sin_panico() {
        let definition = definition(vec![block(
            "raro",
            "code",
            json!({ "a": 1 }),
            json!({ "oneOf": [{ "type": "string" }] }),
        )]);
        let invoker = MockInvoker::new(vec![ok(json!({}))]);
        let executor = Executor::new(invoker);
        let outcome = executor.run(run(&definition)).await;

        assert_eq!(executor.invoker().calls(), 0);
        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::ValidationFailed);
        assert_eq!(error.details.unwrap()["reason"], json!("UNSUPPORTED_SCHEMA"));
    }

    // ── referencias no resueltas ───────────────────────────────────────

    #[tokio::test]
    async fn referencia_no_resuelta_en_el_input_falla_con_mensaje_quirurgico() {
        let definition = definition(vec![
            block("uno", "code-1", json!({}), json!({})),
            block("dos", "code-2", json!({ "x": "$.uno.noExiste" }), json!({})),
            block("tres", "code-3", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![ok(json!({ "ok": true })), ok(json!({})), ok(json!({}))]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::RefUnresolved);
        assert_eq!(error.code.as_str(), "REF_UNRESOLVED");
        assert_eq!(error.block.as_deref(), Some("dos"));
        assert!(error.message.contains("$.uno.noExiste"), "{}", error.message);
        assert!(error.message.contains("noExiste"), "{}", error.message);
        assert_eq!(executor.invoker().calls(), 1);
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(
            outcome.steps[1].input,
            json!({ "x": "$.uno.noExiste" }),
            "el paso fallido guarda el mapeo sin resolver"
        );
    }

    #[tokio::test]
    async fn referencia_no_resuelta_en_la_config_falla_fast_antes_de_invocar() {
        let mut definition = definition(vec![block("http", "code", json!({}), json!({}))]);
        definition.blocks[0].config = json!({ "url": "https://api.x.com/{$.auth.token}" });
        let invoker = MockInvoker::new(vec![ok(json!({}))]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert_eq!(executor.invoker().calls(), 0);
        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::RefUnresolved);
        assert!(error.message.contains("$.auth.token"), "{}", error.message);
    }

    // ── timeout por instancia ──────────────────────────────────────────

    #[tokio::test]
    async fn usa_el_timeout_de_la_instancia_o_el_default_del_ejecutor() {
        let mut definition = definition(vec![
            block("rapido", "code-1", json!({}), json!({})),
            block("lento", "code-2", json!({}), json!({})),
        ]);
        definition.blocks[1].timeout_ms = Some(250);
        let invoker = MockInvoker::new(vec![ok(json!({})), ok(json!({}))]);
        let executor = Executor::new(invoker).with_default_timeout_ms(9_000);

        executor.run(run(&definition)).await;

        assert_eq!(executor.invoker().call(0).2, 9_000);
        assert_eq!(executor.invoker().call(1).2, 250);
    }

    // ── invariantes de la definición ───────────────────────────────────

    #[tokio::test]
    async fn nombres_reservados_o_duplicados_invalidan_la_definicion_sin_ejecutar() {
        for blocks in [
            vec![block("trigger", "code", json!({}), json!({}))],
            vec![block("now", "code", json!({}), json!({}))],
            vec![
                block("repe", "code-1", json!({}), json!({})),
                block("repe", "code-2", json!({}), json!({})),
            ],
        ] {
            let definition = definition(blocks);
            let invoker = MockInvoker::new(vec![]);
            let executor = Executor::new(invoker);
            let outcome = executor.run(run(&definition)).await;

            assert!(!outcome.is_success());
            assert_eq!(executor.invoker().calls(), 0);
            assert!(outcome.steps.is_empty());
            let error = outcome.error.expect("error estructurado");
            assert_eq!(error.code, ErrorCode::ValidationFailed);
            assert!(error.block.is_some());
        }
    }

    #[tokio::test]
    async fn un_bloque_sin_codigo_usa_el_camino_builtin_del_host() {
        let mut definicion = definition(vec![block("builtin", "code", json!({}), json!({}))]);
        definicion.blocks[0].code = None;
        let invoker = MockInvoker::con_builtin(vec![Ok(json!({ "eco": true }))]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definicion)).await;

        // La cadena corrió por el camino built-in: el sandbox no se invocó.
        assert_eq!(executor.invoker().calls(), 0);
        assert_eq!(executor.invoker().builtin_calls(), 1);
        assert_eq!(outcome.status, ExecutionStatus::Succeeded);
        assert_eq!(outcome.output, Some(json!({ "eco": true })));

        // Y un fallo del built-in (red, credencial ausente…) corta la cadena.
        let mut definicion_fallo = definition(vec![block("builtin", "code", json!({}), json!({}))]);
        definicion_fallo.blocks[0].code = None;
        let invoker2 = MockInvoker::con_builtin(vec![Err(SandboxError::HostError {
            message: "la credencial \"x\" no existe en la bóveda del workspace".to_string(),
        })]);
        let executor2 = Executor::new(invoker2);
        let outcome2 = executor2.run(run(&definicion_fallo)).await;
        assert_eq!(outcome2.status, ExecutionStatus::Failed);
        let error = outcome2.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::BlockFailed);
        assert_eq!(error.details.unwrap()["reason"], json!("HOST_ERROR"));
    }
}