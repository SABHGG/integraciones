//! Integración del ejecutor con el sandbox real (rquickjs/quickjs-ng).
//!
//! `sandbox::run_block` no es `Send` (AsyncContext full-async), así que estos
//! tests corren en un runtime current-thread, igual que el worker.

use domain::definition::{BlockInstance, BridgeDefinition, TriggerDefinition, TriggerType};
use domain::execution::{ErrorCode, ExecutionStatus, StepStatus};
use engine::{ChainRun, Executor, SandboxInvoker};
use serde_json::{Value, json};

fn block(name: &str, code: &str, input: Value) -> BlockInstance {
    BlockInstance {
        id: format!("instancia-{name}"),
        name: name.to_string(),
        block_id: None,
        block_version_id: None,
        code: Some(code.to_string()),
        when: None,
        input,
        config: json!({}),
        input_schema: json!({}),
        output_schema: json!({}),
        timeout_ms: None,
    }
}

fn definition(blocks: Vec<BlockInstance>) -> BridgeDefinition {
    BridgeDefinition {
        trigger: TriggerDefinition {
            schedule: None,
            timezone: None,
            topic: None,
            trigger_type: TriggerType::Http,
            method: Some("POST".to_string()),
            path: Some("/x/demo".to_string()),
        },
        input_schema: None,
        blocks,
        webhook: None,
        mask: Vec::new(),
        emit: None,
    }
}

fn run(definition: &BridgeDefinition) -> ChainRun<'_> {
    ChainRun {
        definition,
        trigger: json!({ "body": { "valor": 5 }, "query": {} }),
        now: "2025-01-02T03:04:05Z".to_string(),
    }
}

#[tokio::test]
async fn encadena_dos_bloques_reales_con_salida_nombrada() {
    let definition = definition(vec![
        block(
            "doble",
            "export default (input) => ({ doubled: input.valor * 2 })",
            json!({ "valor": "$.trigger.body.valor" }),
        ),
        block(
            "suma",
            "export default (input) => ({ resultado: input.doubled + 1 })",
            json!({ "doubled": "$.doble.doubled" }),
        ),
    ]);
    let executor = Executor::new(SandboxInvoker::new());

    let outcome = executor.run(run(&definition)).await;

    assert_eq!(
        outcome.status,
        ExecutionStatus::Succeeded,
        "{:?}",
        outcome.error
    );
    assert_eq!(outcome.output, Some(json!({ "resultado": 11 })));
    assert_eq!(outcome.steps.len(), 2);
    assert_eq!(outcome.steps[0].output, Some(json!({ "doubled": 10 })));
    assert_eq!(outcome.steps[1].output, Some(json!({ "resultado": 11 })));
    assert!(
        outcome
            .steps
            .iter()
            .all(|s| s.status == StepStatus::Succeeded)
    );
}

#[tokio::test]
async fn un_throw_real_se_registra_como_bloque_fallido_y_corta_la_cadena() {
    let definition = definition(vec![
        block("ok", "export default () => ({ ok: true })", json!({})),
        block(
            "rompe",
            "export default () => { throw new Error('boom real') }",
            json!({}),
        ),
        block("nunca", "export default () => ({ nunca: true })", json!({})),
    ]);
    let executor = Executor::new(SandboxInvoker::new());

    let outcome = executor.run(run(&definition)).await;

    assert_eq!(outcome.status, ExecutionStatus::Failed);
    assert_eq!(outcome.steps.len(), 2, "el tercer bloque no se ejecuta");
    let error = outcome.error.expect("error estructurado");
    assert_eq!(error.code, ErrorCode::BlockFailed);
    assert_eq!(error.block.as_deref(), Some("rompe"));
    assert!(error.message.contains("boom real"), "{}", error.message);
    assert!(outcome.context.get("nunca").is_none());
}

#[tokio::test]
async fn un_bucle_infinito_real_termina_en_timeout_del_ejecutor() {
    let mut definition = definition(vec![block(
        "bucle",
        "export default () => { while (true) {} }",
        json!({}),
    )]);
    definition.blocks[0].timeout_ms = Some(150);
    let executor = Executor::new(SandboxInvoker::new());

    let outcome = executor.run(run(&definition)).await;

    let error = outcome.error.expect("error estructurado");
    assert_eq!(error.code, ErrorCode::Timeout, "{}", error.message);
    assert_eq!(outcome.steps[0].status, StepStatus::Failed);
    assert!(outcome.steps[0].duration_ms.is_some());
}

#[tokio::test]
async fn una_promise_asincrona_real_encadena_correctamente() {
    let definition = definition(vec![block(
        "asincrono",
        "export default async (input) => ({ esperado: input.valor + 1 })",
        json!({ "valor": "$.trigger.body.valor" }),
    )]);
    let executor = Executor::new(SandboxInvoker::new());

    let outcome = executor.run(run(&definition)).await;

    assert_eq!(outcome.output, Some(json!({ "esperado": 6 })));
}
