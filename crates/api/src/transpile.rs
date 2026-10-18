//! Pipeline TypeScript → JavaScript de la publicación de bloques.
//!
//! Decisión de design.md: "Transpilar con swc al publicar la versión; la versión
//! inmutable guarda el JS resultante. El runtime nunca ve TS". Por tanto esto
//! corre en el plano admin (`POST /blocks/{id}/versions`), nunca en el sandbox.
//!
//! El pipeline es: parsear el módulo como TypeScript → exigir `export default`
//! → `resolver` + `strip` (borra tipos, sin comprobación de tipos: eso es de
//! `tsc`, no del runtime) + `fixer` → emitir JavaScript con target ES2022
//! (subconjunto seguro para QuickJS/quickjs-ng, el motor del sandbox).
//!
//! Los errores de parseo se devuelven como [`TranspileError::Syntax`] con línea
//! y columna, para que el 422 del handler sea accionable.

use swc_core::common::sync::Lrc;
use swc_core::common::{FileName, GLOBALS, Globals, Mark, SourceMap, Spanned};
use swc_core::ecma::ast::{EsVersion, ModuleDecl, ModuleItem, Program};
use swc_core::ecma::codegen::text_writer::JsWriter;
use swc_core::ecma::codegen::{Config as CodegenConfig, Emitter, Node};
use swc_core::ecma::parser::{Syntax, TsSyntax, parse_file_as_program};
use swc_core::ecma::transforms::base::fixer::fixer;
use swc_core::ecma::transforms::base::resolver;
use swc_core::ecma::transforms::typescript::strip;

/// Error del pipeline de transpilación.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranspileError {
    /// El código no es TypeScript/JavaScript válido.
    #[error("el código no compila (línea {line}, columna {column}): {message}")]
    Syntax {
        /// Línea del fallo (1-indexada).
        line: usize,
        /// Columna del fallo (1-indexada).
        column: usize,
        /// Mensaje del parser de swc.
        message: String,
    },
    /// El módulo no declara `export default`.
    #[error(
        "el bloque debe declarar un export default con la firma \
         `export default function(input, ctx)`"
    )]
    MissingDefaultExport,
    /// swc emitió el AST pero el escritor de salida falló (no debería pasar
    /// con un buffer en memoria; sin `expect` para no tumbar el handler).
    #[error("no se pudo generar el JavaScript transpilado: {0}")]
    Codegen(String),
}

/// Transpila el código TypeScript de un bloque a JavaScript.
///
/// Devuelve el módulo JS (con su `export default` intacto, que es lo que carga
/// el sandbox) o el primer error del pipeline.
pub fn transpile_typescript(source: &str) -> Result<String, TranspileError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        FileName::Custom("bloque.ts".into()).into(),
        source.to_string(),
    );

    // `no_early_errors: false` (por defecto) hace que swc acumule los errores
    // recuperables; se tratan como fatales para no guardar JS dudoso.
    let mut recovered = Vec::new();
    let program = parse_file_as_program(
        &fm,
        Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: true,
            dts: false,
            no_early_errors: false,
            disallow_ambiguous_jsx_like: true,
        }),
        EsVersion::Es2022,
        None,
        &mut recovered,
    )
    .map_err(|error| syntax_error(&cm, error.span(), &error.kind().msg()))?;

    if let Some(error) = recovered.first() {
        return Err(syntax_error(&cm, error.span(), &error.kind().msg()));
    }

    if !has_default_export(&program) {
        return Err(TranspileError::MissingDefaultExport);
    }

    // Marca el JS resultante con el target de QuickJS: los marks ya están
    // resueltos por `resolver`, así que el strip de tipos es seguro.
    let emitted = GLOBALS.set(&Globals::default(), || {
        let unresolved_mark = Mark::new();
        let top_level_mark = Mark::new();

        let program = program
            .apply(resolver(unresolved_mark, top_level_mark, true))
            .apply(strip(unresolved_mark, top_level_mark))
            .apply(fixer(None));

        emit(&cm, &program)
    });

    emitted.map_err(|e| TranspileError::Codegen(e.to_string()))
}

/// Comprueba por AST que el módulo declara `export default`.
///
/// La forma (función, expresión, clase) no se inspecciona: el contrato de
/// invocación `(input, ctx)` lo verifica el ejecutor al llamar al bloque.
fn has_default_export(program: &Program) -> bool {
    let Program::Module(module) = program else {
        return false;
    };
    module.body.iter().any(|item| {
        matches!(
            item,
            ModuleItem::ModuleDecl(
                ModuleDecl::ExportDefaultDecl(_) | ModuleDecl::ExportDefaultExpr(_)
            )
        )
    })
}

/// Construye un [`TranspileError::Syntax`] con la posición 1-indexada dentro del
/// fuente (el span de swc es global al `SourceMap`).
fn syntax_error(cm: &SourceMap, span: swc_core::common::Span, message: &str) -> TranspileError {
    let loc = cm.lookup_char_pos(span.lo);
    TranspileError::Syntax {
        line: loc.line,
        column: loc.col_display + 1,
        message: message.to_string(),
    }
}

/// Emite el programa a JavaScript con target ES2022 (subconjunto seguro para
/// QuickJS/quickjs-ng, el motor del sandbox).
fn emit(cm: &Lrc<SourceMap>, program: &Program) -> Result<String, std::io::Error> {
    let mut buf = Vec::new();
    {
        let cfg = CodegenConfig::default()
            .with_target(EsVersion::Es2022)
            .with_minify(false);
        let mut emitter = Emitter {
            cfg,
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm.clone(), "\n", &mut buf, None),
        };
        program.emit_with(&mut emitter)?;
    }
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Bloque de ejemplo del design.md: función única con export por defecto.
    const BLOQUE_TS: &str = r#"
type Entrada = { userId: string };

export default function (input: Entrada, ctx: any) {
  const nombre: string = input.userId.toUpperCase();
  return { nombre };
}
"#;

    async fn ejecutar_js(js: &str, input: serde_json::Value) -> serde_json::Value {
        let config = sandbox::SandboxConfig::default();
        sandbox::run_block(js, input, config)
            .await
            .unwrap_or_else(|e| panic!("el JS transpilado debe ejecutarse en el sandbox: {e}"))
    }

    #[test]
    fn transpila_typescript_valido_y_borra_los_tipos() {
        let js = transpile_typescript(BLOQUE_TS).expect("bloque válido");

        assert!(
            js.contains("export default"),
            "se pierde el export default: {js}"
        );
        assert!(!js.contains("type Entrada"), "quedaron tipos: {js}");
        assert!(!js.contains(": string"), "quedaron anotaciones: {js}");
        assert!(!js.contains(": Entrada"), "quedaron anotaciones: {js}");
    }

    #[tokio::test]
    async fn el_js_transpilado_es_cargable_por_el_sandbox() {
        let js = transpile_typescript(BLOQUE_TS).expect("bloque válido");
        let salida = ejecutar_js(&js, json!({ "userId": "ana" })).await;
        assert_eq!(salida, json!({ "nombre": "ANA" }));
    }

    #[test]
    fn transpila_una_funcion_async_con_enum_interfaces_y_genericos() {
        let source = r#"
interface Config { url: string }
enum Modo { A = "a", B = "b" }
async function construir<T extends Config>(cfg: T): Promise<T> { return cfg; }

export default async function (input: { userId: string }, ctx: any) {
  const cfg = await construir({ url: `https://x/${input.userId}` });
  return { modo: Modo.A as string, url: cfg.url };
}
"#;
        let js = transpile_typescript(source).expect("TS válido");

        assert!(js.contains("export default"), "{js}");
        assert!(js.contains("async"), "se pierde async: {js}");
        assert!(!js.contains("interface"), "quedó una interface: {js}");
        assert!(!js.contains("Promise<T>"), "quedaron genéricos: {js}");
        assert!(!js.contains(": string"), "quedaron anotaciones: {js}");
        assert!(js.contains("Modo"), "el enum debe sobrevivir: {js}");
    }

    #[tokio::test]
    async fn acepta_un_export_default_de_expresion_flecha() {
        let js =
            transpile_typescript("export default (input: any) => ({ ok: true, id: input.id });")
                .expect("export default válido");

        let salida = ejecutar_js(&js, json!({ "id": 7 })).await;
        assert_eq!(salida, json!({ "ok": true, "id": 7 }));
    }

    #[test]
    fn un_error_de_sintaxis_reporta_linea_y_columna_sin_crear_version() {
        let source = "export default function () {\n  const roto: = ;\n}\n";
        match transpile_typescript(source) {
            Err(TranspileError::Syntax {
                line,
                column,
                message,
                ..
            }) => {
                assert_eq!(line, 2, "la línea del fallo debe ser la 2: {message}");
                assert!(column > 0, "la columna debe ser 1-indexada: {column}");
                assert!(!message.is_empty());
            }
            other => panic!("se esperaba Syntax, llegó {other:?}"),
        }
    }

    #[test]
    fn un_codigo_que_no_es_ni_javascript_es_un_error_de_sintaxis() {
        assert!(matches!(
            transpile_typescript("function ( {"),
            Err(TranspileError::Syntax { .. })
        ));
    }

    #[test]
    fn rechaza_un_modulo_sin_export_default() {
        for source in [
            "export function bloque(input: any) { return input; }",
            "const x = 1;",
            "",
            "export { x };\nconst x = 1;",
        ] {
            assert_eq!(
                transpile_typescript(source),
                Err(TranspileError::MissingDefaultExport),
                "fuente: {source:?}"
            );
        }
    }

    #[test]
    fn exige_export_default_pero_no_la_forma_concreta() {
        // La forma (función con default, expresión, clase) no se inspecciona:
        // el contrato de invocación lo verifica el ejecutor al llamar.
        assert!(transpile_typescript("export default class Bloque {}").is_ok());
        assert!(transpile_typescript("export default 42;").is_ok());
    }

    #[test]
    fn los_tipos_no_se_comprueban_al_publicar() {
        // `strip` borra anotaciones sin type-checking (eso es trabajo de `tsc`):
        // publicar tipos imposibles no falla, solo se eliminan.
        let js = transpile_typescript("const x: NoExiste = 1; export default () => x;")
            .expect("sin type-checking");
        assert!(!js.contains("NoExiste"), "{js}");
    }

    #[test]
    fn un_error_recuperable_del_parser_tambien_rechaza_la_publicacion() {
        // swc parsea y deja los errores de contexto (early errors) en
        // `recovered`: un `break` fuera de bucle, una etiqueta duplicada o un
        // constructor repetido llegan ahí. Se tratan como fatales para no
        // publicar JS que el sandbox cargaría pero rompería al ejecutar.
        match transpile_typescript("export default () => { break; };") {
            Err(TranspileError::Syntax {
                line,
                column,
                message,
                ..
            }) => {
                assert_eq!(line, 1);
                assert!(column > 0);
                assert!(message.contains("break"), "mensaje: {message}");
            }
            other => panic!("se esperaba Syntax, llegó {other:?}"),
        }
    }

    #[test]
    fn la_transpilacion_es_determinista() {
        let primera = transpile_typescript(BLOQUE_TS).expect("válido");
        let segunda = transpile_typescript(BLOQUE_TS).expect("válido");
        assert_eq!(primera, segunda);
    }
}
