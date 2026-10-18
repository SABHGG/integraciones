//! Máquina de ejecución de la cadena lineal de bloques.
//!
//! Recorre las instancias de `bridge_versions.definition` en orden, resuelve el
//! input y la config de cada bloque contra el contexto acumulado, valida la
//! entrada contra el `inputSchema` del bloque, invoca el motor de bloques y
//! registra la salida **nombrada** en el contexto. El primer error termina la
//! ejecución (fail-fast) y queda registrado en el trace con el bloque culpable.

use std::collections::HashSet;
use std::time::Instant;

use domain::conditions;
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
        // Objetivos de rutas `switch` no tomadas: cuando el flujo lineal llega
        // a uno de ellos, se registra `skipped` sin ejecutarlo.
        let mut non_chosen: HashSet<String> = HashSet::new();
        let total = run.definition.blocks.len();
        let mut index = 0usize;

        while index < total {
            let block = &run.definition.blocks[index];
            let seq = index as i32 + 1;

            // 0) Objetivo de una ruta `switch` no tomada: `skipped` en su seq.
            if non_chosen.contains(&block.id) {
                steps.push(skipped_step(seq, block));
                index += 1;
                continue;
            }

            // 1) `when`: la condición decide si la instancia corre.
            if let Some(condition) = &block.when {
                match conditions::evaluate(&context, condition) {
                    Ok(true) => {}
                    Ok(false) => {
                        steps.push(skipped_step(seq, block));
                        index += 1;
                        continue;
                    }
                    Err(error) => {
                        let step = failed_step(
                            seq,
                            block,
                            json!({}),
                            condition_error(block, error),
                            None,
                        );
                        return failed_outcome(steps, step, context);
                    }
                }
            }

            // 2) Built-in `switch`: enruta la cadena sin invocar el sandbox.
            // Contrato: instancia sin código cuya config declara `routes`
            // (array de {when, to}) con `default` opcional (id de instancia
            // posterior). La validación de publicación garantiza la forma.
            let is_switch = block.code.is_none() && block.config.get("routes").is_some();
            if is_switch {
                let routes = match parse_routes(block) {
                    Ok(routes) => routes,
                    Err(error) => {
                        let step = failed_step(seq, block, json!({}), error, None);
                        return failed_outcome(steps, step, context);
                    }
                };

                // Primera condición verdadera, en orden de declaración; el
                // resto NO se evalúa.
                let mut chosen: Option<String> = None;
                for route in &routes {
                    match conditions::evaluate(&context, &route.when) {
                        Ok(true) => {
                            chosen = Some(route.to.clone());
                            break;
                        }
                        Ok(false) => continue,
                        Err(error) => {
                            let step = failed_step(
                                seq,
                                block,
                                json!({}),
                                condition_error(block, error),
                                None,
                            );
                            return failed_outcome(steps, step, context);
                        }
                    }
                }
                if chosen.is_none() {
                    chosen = block
                        .config
                        .get("default")
                        .and_then(Json::as_str)
                        .map(str::to_string);
                }

                // El objetivo debe existir y ser posterior (sin ciclos).
                let jump_to = match &chosen {
                    Some(target_id) => {
                        match run.definition.blocks.iter().position(|b| &b.id == target_id) {
                            Some(at) if at > index => Some(at),
                            _ => {
                                let error = ErrorPayload::new(
                                    ErrorCode::ValidationFailed,
                                    format!(
                                        "la ruta del switch \"{}\" apunta a \"{target_id}\", que no existe o no es posterior en la cadena",
                                        block.name
                                    ),
                                )
                                .with_block(block.name.clone())
                                .with_details(json!({ "reason": "SWITCH_TARGET_INVALID" }));
                                let step = failed_step(seq, block, json!({}), error, None);
                                return failed_outcome(steps, step, context);
                            }
                        }
                    }
                    None => None,
                };

                // El switch registra su decisión (útil en el trace) sin IO.
                let decision = json!({ "to": chosen });
                if let Json::Object(map) = &mut context {
                    map.insert(block.name.clone(), decision.clone());
                }
                last_output = Some(decision.clone());
                steps.push(ExecutionStep {
                    seq,
                    block_instance_id: block.id.clone(),
                    block_name: block.name.clone(),
                    status: StepStatus::Succeeded,
                    input: block.config.clone(),
                    output: Some(decision),
                    error: None,
                    duration_ms: None,
                });

                // Los objetivos no tomados (incluido el default no elegido)
                // quedan `skipped` cuando el flujo lineal los alcance.
                for route in &routes {
                    if chosen.as_deref() != Some(route.to.as_str()) {
                        non_chosen.insert(route.to.clone());
                    }
                }
                if let Some(default_to) = block.config.get("default").and_then(Json::as_str)
                    && chosen.as_deref() != Some(default_to)
                {
                    non_chosen.insert(default_to.to_string());
                }

                // Salto: los bloques intermedios hasta el objetivo se marcan
                // `skipped` (conservan su seq original) y la ejecución sigue
                // en el objetivo.
                index = match jump_to {
                    Some(j) => {
                        for bypassed in (index + 1)..j {
                            steps.push(skipped_step(
                                bypassed as i32 + 1,
                                &run.definition.blocks[bypassed],
                            ));
                        }
                        j
                    }
                    None => index + 1,
                };
                continue;
            }

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

            index += 1;
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
fn failed_outcome(
    steps: Vec<ExecutionStep>,
    step: ExecutionStep,
    context: Json,
) -> ExecutionOutcome {
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

/// Construye un paso `skipped`: bloque no ejecutado por enrutado condicional,
/// en su seq original y sin input ejecutado, salida, error ni duración.
fn skipped_step(seq: i32, block: &BlockInstance) -> ExecutionStep {
    ExecutionStep {
        seq,
        block_instance_id: block.id.clone(),
        block_name: block.name.clone(),
        status: StepStatus::Skipped,
        input: json!({}),
        output: None,
        error: None,
        duration_ms: None,
    }
}

/// Traduce un fallo de condición al contrato de error: referencia no resuelta
/// es `REF_UNRESOLVED` (identificando la ref); una condición malformada o con
/// operandos incomparables es fail-closed con `VALIDATION_FAILED`.
fn condition_error(block: &BlockInstance, error: conditions::ConditionError) -> ErrorPayload {
    match error {
        conditions::ConditionError::Unresolved { raw, detail } => ErrorPayload::new(
            ErrorCode::RefUnresolved,
            format!(
                "la condición del bloque \"{}\" referenció {raw}, que no resolvió: {detail}",
                block.name
            ),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "CONDITION_REF_UNRESOLVED", "ref": raw })),
        other => ErrorPayload::new(
            ErrorCode::ValidationFailed,
            format!(
                "la condición del bloque \"{}\" no es válida: {other}",
                block.name
            ),
        )
        .with_block(block.name.clone())
        .with_details(json!({ "reason": "CONDITION_INVALID" })),
    }
}

/// Una ruta de un switch: condición e instancia destino.
struct SwitchRoute {
    when: Json,
    to: String,
}

/// Error de la forma del switch del bloque.
fn invalid_switch(block: &BlockInstance, reason: &str) -> ErrorPayload {
    ErrorPayload::new(
        ErrorCode::ValidationFailed,
        format!(
            "la config del switch \"{}\" no es válida: {reason}",
            block.name
        ),
    )
    .with_block(block.name.clone())
    .with_details(json!({ "reason": "SWITCH_INVALID" }))
}

/// Parsea las rutas del switch desde la config: `routes: [{when, to}]`.
fn parse_routes(block: &BlockInstance) -> Result<Vec<SwitchRoute>, ErrorPayload> {
    let Some(items) = block.config.get("routes").and_then(Json::as_array) else {
        return Err(invalid_switch(
            block,
            "\"routes\" debe ser un array de objetos {when, to}",
        ));
    };
    let mut routes = Vec::with_capacity(items.len());
    for item in items {
        let Some(when) = item.get("when").filter(|value| value.is_object()) else {
            return Err(invalid_switch(
                block,
                "cada ruta necesita \"when\" (un objeto de la gramática declarativa)",
            ));
        };
        let Some(to) = item.get("to").and_then(Json::as_str) else {
            return Err(invalid_switch(
                block,
                "cada ruta necesita \"to\" (el id de la instancia destino)",
            ));
        };
        routes.push(SwitchRoute {
            when: when.clone(),
            to: to.to_string(),
        });
    }
    Ok(routes)
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
    ErrorPayload::new(ErrorCode::RefUnresolved, error.to_string()).with_block(block.name.clone())
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
            when: None,
            input,
            config: json!({}),
            input_schema,
            output_schema: json!({}),
            timeout_ms: None,
        }
    }

    /// Instancia con condición `when` declarativa.
    fn con_when(mut block: BlockInstance, when: Value) -> BlockInstance {
        block.when = Some(when);
        block
    }

    /// Bloque built-in `switch`: sin código, config con `routes` (+ `default`).
    fn switch_block(name: &str, routes: Value, default: Option<&str>) -> BlockInstance {
        let mut config = json!({ "routes": routes });
        if let Some(to) = default {
            config["default"] = json!(to);
        }
        BlockInstance {
            id: format!("instancia-{name}"),
            name: name.to_string(),
            block_id: None,
            block_version_id: None,
            code: None,
            when: None,
            input: json!({}),
            config,
            input_schema: json!({}),
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
                schedule: None,
                timezone: None,
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
        let invoker =
            MockInvoker::new(vec![ok(json!({ "id": "u-1" })), ok(json!({ "total": 20 }))]);
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
        assert_eq!(
            executor.invoker().calls(),
            2,
            "el tercer bloque no se invoca"
        );
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
            block("origen", "code-1", json!({}), json!({})),
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
        assert_eq!(
            error.details.unwrap()["reason"],
            json!("UNSUPPORTED_SCHEMA")
        );
    }

    // ── referencias no resueltas ───────────────────────────────────────

    #[tokio::test]
    async fn referencia_no_resuelta_en_el_input_falla_con_mensaje_quirurgico() {
        let definition = definition(vec![
            block("uno", "code-1", json!({}), json!({})),
            block("dos", "code-2", json!({ "x": "$.uno.noExiste" }), json!({})),
            block("tres", "code-3", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![
            ok(json!({ "ok": true })),
            ok(json!({})),
            ok(json!({})),
        ]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::RefUnresolved);
        assert_eq!(error.code.as_str(), "REF_UNRESOLVED");
        assert_eq!(error.block.as_deref(), Some("dos"));
        assert!(
            error.message.contains("$.uno.noExiste"),
            "{}",
            error.message
        );
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

    // ── enrutado condicional (MVP2) ───────────────────────────────────

    #[tokio::test]
    async fn un_when_falso_marca_el_paso_como_skipped_y_la_cadena_continua() {
        let medio = con_when(
            block("medio", "code-medio", json!({}), json!({})),
            json!({ "eq": ["$.trigger.body.cantidad", 99] }), // falso: cantidad = 2
        );
        let definition = definition(vec![
            block("uno", "code-1", json!({}), json!({})),
            medio,
            block("tres", "code-3", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![ok(json!({ "paso": 1 })), ok(json!({ "paso": 3 }))]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(outcome.is_success(), "{:?}", outcome.error);
        assert_eq!(executor.invoker().calls(), 2, "el bloque saltado no se invoca");
        assert_eq!(outcome.steps.len(), 3);
        assert_eq!(outcome.steps[1].seq, 2);
        assert_eq!(outcome.steps[1].block_name, "medio");
        assert_eq!(outcome.steps[1].status, StepStatus::Skipped);
        assert_eq!(outcome.steps[1].input, json!({}), "sin input ejecutado");
        assert_eq!(outcome.steps[1].output, None);
        assert_eq!(outcome.steps[1].error, None);
        assert_eq!(outcome.steps[1].duration_ms, None, "sin duración");
        assert!(outcome.context.get("medio").is_none(), "no produce salida");
        assert_eq!(outcome.output, Some(json!({ "paso": 3 })));
    }

    #[tokio::test]
    async fn un_when_verdadero_se_ejecuta_normalmente() {
        let condicionado = con_when(
            block("extra", "code-extra", json!({}), json!({})),
            json!({ "gte": ["$.trigger.body.cantidad", 1] }),
        );
        let definition = definition(vec![condicionado]);
        let invoker = MockInvoker::new(vec![ok(json!({ "ok": true }))]);
        let outcome = Executor::new(invoker).run(run(&definition)).await;

        assert!(outcome.is_success());
        assert_eq!(outcome.steps[0].status, StepStatus::Succeeded);
        assert_eq!(outcome.context["extra"], json!({ "ok": true }));
    }

    #[tokio::test]
    async fn una_condicion_con_referencia_no_resuelta_falla_fast_con_ref_unresolved() {
        let condicionado = con_when(
            block("cond", "code-cond", json!({}), json!({})),
            json!({ "exists": "$.cliente.pais" }),
        );
        let definition = definition(vec![condicionado]);
        let invoker = MockInvoker::new(vec![ok(json!({}))]);
        let executor = Executor::new(invoker);
        let outcome = executor.run(run(&definition)).await;

        assert_eq!(executor.invoker().calls(), 0, "la condición falla antes de invocar");
        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::RefUnresolved);
        assert_eq!(error.code.as_str(), "REF_UNRESOLVED");
        assert_eq!(error.block.as_deref(), Some("cond"));
        assert!(error.message.contains("$.cliente.pais"), "{}", error.message);
        assert_eq!(outcome.steps[0].status, StepStatus::Failed);
    }

    #[tokio::test]
    async fn un_switch_enruta_por_la_primera_condicion_verdadera() {
        let switch = switch_block(
            "switch",
            json!([
                { "when": { "gte": ["$.trigger.body.cantidad", 10] }, "to": "instancia-vip" },
                { "when": { "eq": ["$.trigger.body.cantidad", 2] }, "to": "instancia-normal" }
            ]),
            Some("instancia-normal"),
        );
        let definition = definition(vec![
            switch,
            block("vip", "code-vip", json!({}), json!({})),
            block("normal", "code-normal", json!({}), json!({})),
        ]);
        // cantidad = 2: la ruta vip no aplica, la normal sí.
        let invoker = MockInvoker::new(vec![ok(json!({ "ruta": "normal" }))]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(outcome.is_success(), "{:?}", outcome.error);
        assert_eq!(executor.invoker().calls(), 1, "solo la ruta tomada se invoca");
        assert_eq!(outcome.steps.len(), 3);
        // El switch registra su decisión sin invocar el sandbox.
        assert_eq!(outcome.steps[0].status, StepStatus::Succeeded);
        assert_eq!(outcome.steps[0].duration_ms, None);
        assert_eq!(outcome.steps[0].output, Some(json!({ "to": "instancia-normal" })));
        // La ruta vip queda skipped en su seq original.
        assert_eq!(outcome.steps[1].block_name, "vip");
        assert_eq!(outcome.steps[1].status, StepStatus::Skipped);
        assert_eq!(outcome.steps[2].block_name, "normal");
        assert_eq!(outcome.steps[2].status, StepStatus::Succeeded);
        assert_eq!(outcome.output, Some(json!({ "ruta": "normal" })));
    }

    #[tokio::test]
    async fn un_switch_sin_ruta_verdadera_usa_default() {
        let switch = switch_block(
            "switch",
            json!([
                { "when": { "gte": ["$.trigger.body.cantidad", 10] }, "to": "instancia-vip" },
                { "when": { "gte": ["$.trigger.body.cantidad", 5] }, "to": "instancia-media" }
            ]),
            Some("instancia-normal"),
        );
        let definition = definition(vec![
            switch,
            block("vip", "code-vip", json!({}), json!({})),
            block("media", "code-media", json!({}), json!({})),
            block("normal", "code-normal", json!({}), json!({})),
        ]);
        // cantidad = 2: ninguna ruta aplica → default (saltando los intermedios).
        let invoker = MockInvoker::new(vec![ok(json!({ "ruta": "default" }))]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(outcome.is_success(), "{:?}", outcome.error);
        assert_eq!(executor.invoker().calls(), 1);
        assert_eq!(outcome.steps[0].output, Some(json!({ "to": "instancia-normal" })));
        assert_eq!(outcome.steps[1].status, StepStatus::Skipped, "vip (ruta no tomada)");
        assert_eq!(outcome.steps[2].status, StepStatus::Skipped, "media (intermedio saltado)");
        assert_eq!(outcome.steps[3].block_name, "normal");
        assert_eq!(outcome.steps[3].status, StepStatus::Succeeded);
    }

    #[tokio::test]
    async fn el_switch_salta_hacia_delante_y_marca_los_intermedios_como_skipped() {
        let switch = switch_block(
            "switch",
            json!([{ "when": { "exists": "$.trigger.body.userId" }, "to": "instancia-destino" }]),
            None,
        );
        let definition = definition(vec![
            switch,
            block("medio", "code-medio", json!({}), json!({})),
            block("destino", "code-destino", json!({}), json!({})),
            block("despues", "code-despues", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![
            ok(json!({ "en": "destino" })),
            ok(json!({ "en": "despues" })),
        ]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(outcome.is_success(), "{:?}", outcome.error);
        assert_eq!(executor.invoker().calls(), 2, "medio no se invoca");
        assert_eq!(outcome.steps[1].block_name, "medio");
        assert_eq!(outcome.steps[1].status, StepStatus::Skipped);
        assert_eq!(outcome.steps[2].block_name, "destino");
        assert_eq!(outcome.steps[2].status, StepStatus::Succeeded);
        // La cadena sigue lineal tras la ruta tomada.
        assert_eq!(outcome.steps[3].block_name, "despues");
        assert_eq!(outcome.steps[3].status, StepStatus::Succeeded);
    }

    #[tokio::test]
    async fn fail_fast_en_la_ruta_tomada_del_switch() {
        let switch = switch_block(
            "switch",
            json!([{ "when": { "exists": "$.trigger.body.userId" }, "to": "instancia-destino" }]),
            None,
        );
        let definition = definition(vec![
            switch,
            block("destino", "throw", json!({}), json!({})),
            block("despues", "code-despues", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![
            Err(SandboxError::JsError { message: "boom".to_string() }),
            ok(json!({ "nunca": true })),
        ]);
        let executor = Executor::new(invoker);

        let outcome = executor.run(run(&definition)).await;

        assert!(!outcome.is_success());
        assert_eq!(executor.invoker().calls(), 1, "fail-fast: nada tras el error");
        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::BlockFailed);
        assert_eq!(error.block.as_deref(), Some("destino"));
    }

    #[tokio::test]
    async fn un_switch_con_condicion_no_resuelta_falla_fast_identificando_la_ruta() {
        let switch = switch_block(
            "switch",
            json!([
                { "when": { "eq": ["$.trigger.body.cantidad", 999] }, "to": "instancia-vip" },
                { "when": { "eq": ["$.cliente.pais", "ES"] }, "to": "instancia-normal" }
            ]),
            None,
        );
        let definition = definition(vec![switch, block("vip", "c", json!({}), json!({}))]);
        let invoker = MockInvoker::new(vec![ok(json!({}))]);
        let executor = Executor::new(invoker);
        let outcome = executor.run(run(&definition)).await;

        assert!(!outcome.is_success());
        assert_eq!(executor.invoker().calls(), 0);
        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::RefUnresolved);
        assert_eq!(error.block.as_deref(), Some("switch"));
        assert!(error.message.contains("$.cliente.pais"), "{}", error.message);
    }

    #[tokio::test]
    async fn un_when_falso_sobre_el_switch_lo_salta_completo() {
        let mut switch = switch_block(
            "switch",
            json!([{ "when": { "exists": "$.trigger.body.userId" }, "to": "instancia-vip" }]),
            None,
        );
        switch.when = Some(json!({ "eq": ["$.trigger.body.cantidad", 999] }));
        let definition = definition(vec![
            switch,
            block("vip", "code-vip", json!({}), json!({})),
        ]);
        let invoker = MockInvoker::new(vec![ok(json!({ "vip": true }))]);
        let outcome = Executor::new(invoker).run(run(&definition)).await;

        // El switch saltado no decide nada: la cadena fluye lineal.
        assert!(outcome.is_success(), "{:?}", outcome.error);
        assert_eq!(outcome.steps[0].status, StepStatus::Skipped);
        assert_eq!(outcome.steps[1].block_name, "vip");
        assert_eq!(outcome.steps[1].status, StepStatus::Succeeded);
    }

    #[tokio::test]
    async fn un_switch_con_ruta_malformada_falla_cerrado() {
        let switch = switch_block("switch", json!("no-soy-un-array"), None);
        let definition = definition(vec![switch]);
        let outcome = Executor::new(MockInvoker::new(vec![]))
            .run(run(&definition))
            .await;

        assert!(!outcome.is_success());
        let error = outcome.error.expect("error estructurado");
        assert_eq!(error.code, ErrorCode::ValidationFailed);
        assert_eq!(error.details.unwrap()["reason"], json!("SWITCH_INVALID"));
    }

    // ── timeout por instancia ──────────────────────────────────

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
