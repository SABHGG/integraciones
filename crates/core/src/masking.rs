//! Enmascaramiento declarativo del IO observable (MVP3, spec `io-masking`).
//!
//! El autor del puente declara rutas `$.` sensibles (`mask`) y el sistema
//! sustituye los subárboles cubiertos por `***` de forma IRREVERSIBLE justo
//! antes de persistir: el valor original nunca toca el store y no existe
//! flag ni endpoint de revelado. Las rutas usan la gramática `$.` existente
//! ([`crate::refs`]) — sin parser nuevo.
//!
//! Semántica de las rutas (la forma del contexto del motor): `$.trigger.rest`
//! cubre el trigger persistido y los inputs donde el valor se interpoló;
//! `$.<bloque>.rest` cubre la salida del bloque (el contexto resuelve el
//! bloque directo). Las reglas viven en la definición y viajan congeladas en
//! el snapshot de la versión: editarlas solo afecta a ejecuciones posteriores.

use crate::refs::{RefPath, Segment};
use serde_json::Value;

/// Valor sustituto: fijo y sin información del original.
pub const MASCARA: &str = "***";

/// Una regla de mask ya validada: raíz (`trigger` o nombre de bloque) y el
/// resto de segmentos que descienden dentro de ese documento.
#[derive(Debug, Clone, PartialEq)]
pub struct ReglaMask {
    /// Raíz de la regla: `"trigger"` o el nombre de un bloque.
    pub raiz: String,
    /// Segmentos debajo de la raíz (vacío = el documento entero).
    pub segmentos: Vec<Segment>,
}

/// Parsea las reglas declarativas de una definición. Devuelve error si
/// alguna ruta no respeta la gramática `$.` (el validador de publicación
/// también las filtra; esto es defensa en profundidad para snapshots).
pub fn parsear_mask(mask: &[String]) -> Result<Vec<ReglaMask>, String> {
    mask.iter()
        .map(|raw| {
            let path = RefPath::parse(raw).map_err(|error| error.to_string())?;
            let mut segmentos = path.segments().to_vec();
            let primera = segmentos.remove(0);
            let Segment::Key(raiz) = primera else {
                return Err(format!(
                    "la raíz de \"{raw}\" debe ser una clave, no un índice"
                ));
            };
            Ok(ReglaMask { raiz, segmentos })
        })
        .collect()
}

/// Sustituye de forma irreversible el subárbol de `doc` que queda en
/// `segmentos` por [`MASCARA`]. Devuelve `true` si enmascaró algo; una ruta
/// que no existe (o atraviesa un tipo no objeto/array) es un no-op.
pub fn enmascarar_ruta(doc: &mut Value, segmentos: &[Segment]) -> bool {
    let mut actual = doc;
    for (indice, segmento) in segmentos.iter().enumerate() {
        let es_ultimo = indice + 1 == segmentos.len();
        let hijo = match (segmento, actual) {
            (Segment::Key(clave), Value::Object(mapa)) => mapa.get_mut(clave.as_str()),
            (Segment::Index(posicion), Value::Array(lista)) => lista.get_mut(*posicion),
            _ => return false,
        };
        let Some(hijo) = hijo else { return false };
        if es_ultimo {
            *hijo = Value::String(MASCARA.to_string());
            return true;
        }
        actual = hijo;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn segmentos(raw: &str) -> Vec<Segment> {
        let mut todos = RefPath::parse(raw)
            .expect("ruta válida")
            .segments()
            .to_vec();
        todos.remove(0);
        todos
    }

    #[test]
    fn enmascara_un_subarbol_y_deja_el_resto_integro() {
        let mut doc = json!({ "cliente": { "tarjeta": "4242", "titular": "Ada" } });
        assert!(enmascarar_ruta(&mut doc, &segmentos("$.x.cliente.tarjeta")));
        assert_eq!(doc["cliente"]["tarjeta"], json!("***"));
        assert_eq!(doc["cliente"]["titular"], json!("Ada"));
    }

    #[test]
    fn enmascara_por_indice_de_array() {
        let mut doc = json!({ "items": [{ "sku": "A-1" }, { "sku": "B-2" }] });
        assert!(enmascarar_ruta(&mut doc, &segmentos("$.x.items.1.sku")));
        assert_eq!(doc["items"][0]["sku"], json!("A-1"));
        assert_eq!(doc["items"][1]["sku"], json!("***"));
    }

    #[test]
    fn una_ruta_inexistente_es_un_no_op() {
        let mut doc = json!({ "cliente": { "titular": "Ada" } });
        assert!(!enmascarar_ruta(
            &mut doc,
            &segmentos("$.x.cliente.tarjeta")
        ));
        assert!(!enmascarar_ruta(
            &mut doc,
            &segmentos("$.x.cliente.titular.profundo")
        ));
        assert_eq!(doc, json!({ "cliente": { "titular": "Ada" } }));
    }

    #[test]
    fn la_sustitucion_es_irreversible() {
        let mut doc = json!({ "secreto": "valor-original-123" });
        assert!(enmascarar_ruta(&mut doc, &segmentos("$.x.secreto")));
        let texto = serde_json::to_string(&doc).expect("serializa");
        assert!(!texto.contains("valor-original-123"), "{texto}");
    }

    #[test]
    fn parsea_reglas_con_raiz_y_segmentos() {
        let reglas = parsear_mask(&["$.trigger.password".into(), "$.consulta.tarjeta".into()])
            .expect("reglas válidas");
        assert_eq!(reglas[0].raiz, "trigger");
        assert_eq!(reglas[0].segmentos, segmentos("$.x.password"));
        assert_eq!(reglas[1].raiz, "consulta");
        assert_eq!(reglas[1].segmentos, segmentos("$.x.tarjeta"));
    }

    #[test]
    fn rechaza_rutas_fuera_de_la_gramatica() {
        assert!(parsear_mask(&["trigger.password".into()]).is_err());
        assert!(parsear_mask(&["$..punto".into()]).is_err());
    }
}
