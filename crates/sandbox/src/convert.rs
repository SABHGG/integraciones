//! Conversión entre `serde_json::Value` y valores de QuickJS.
//!
//! `rquickjs` no trae integración con `serde_json`, así que el sandbox expone
//! estos conversores: entrada y salida de los bloques son JSON plano.

use rquickjs::{Array, Ctx, Object, Result, Value};
use serde_json::Value as Json;

/// Convierte un valor JSON en un valor JavaScript dentro de `ctx`.
pub(crate) fn json_to_js<'js>(ctx: &Ctx<'js>, value: &Json) -> Result<Value<'js>> {
    Ok(match value {
        Json::Null => Value::new_null(ctx.clone()),
        Json::Bool(b) => Value::new_bool(ctx.clone(), *b),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    Value::new_int(ctx.clone(), i as i32)
                } else {
                    Value::new_float(ctx.clone(), i as f64)
                }
            } else if let Some(u) = n.as_u64() {
                if u <= i32::MAX as u64 {
                    Value::new_int(ctx.clone(), u as i32)
                } else {
                    Value::new_float(ctx.clone(), u as f64)
                }
            } else {
                Value::new_float(ctx.clone(), n.as_f64().unwrap_or(f64::NAN))
            }
        }
        Json::String(s) => rquickjs::String::from_str(ctx.clone(), s)?.into_value(),
        Json::Array(items) => {
            let array = Array::new(ctx.clone())?;
            for (index, item) in items.iter().enumerate() {
                array.set(index, json_to_js(ctx, item)?)?;
            }
            array.into_value()
        }
        Json::Object(map) => {
            let object = Object::new(ctx.clone())?;
            for (key, item) in map {
                object.set(key.as_str(), json_to_js(ctx, item)?)?;
            }
            object.into_value()
        }
    })
}

/// Convierte un valor JavaScript en un valor JSON.
///
/// `undefined`, funciones y símbolos se degradan a `null` porque JSON no los
/// representa; el contrato de bloques trabaja solo con datos JSON.
pub(crate) fn js_to_json<'js>(value: &Value<'js>) -> Result<Json> {
    if value.is_null() || value.is_undefined() {
        return Ok(Json::Null);
    }
    if let Some(b) = value.as_bool() {
        return Ok(Json::Bool(b));
    }
    if let Some(i) = value.as_int() {
        return Ok(Json::from(i));
    }
    if let Some(f) = value.as_float() {
        return Ok(Json::from(f));
    }
    if let Some(s) = value.as_string() {
        return Ok(Json::String(s.to_string()?));
    }
    if let Some(array) = value.as_array() {
        let mut out = Vec::with_capacity(array.len());
        for item in array.iter::<Value>() {
            out.push(js_to_json(&item?)?);
        }
        return Ok(Json::Array(out));
    }
    if let Some(object) = value.as_object() {
        let mut map = serde_json::Map::new();
        for entry in object.props::<String, Value>() {
            let (key, item) = entry?;
            map.insert(key, js_to_json(&item)?);
        }
        return Ok(Json::Object(map));
    }
    Ok(Json::Null)
}
