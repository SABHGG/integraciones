//! Cron estándar de 5 campos con timezone IANA.
//!
//! Un trigger cron declara `{ "schedule": "0 9 * * 1-5", "timezone": "Europe/Madrid" }`.
//! El parser valida la expresión (minuto, hora, día del mes, mes y día de la
//! semana) y calcula las próximas ocurrencias **en la timezone declarada**,
//! respetando los horarios de verano:
//!
//! - una hora local que no existe (salto de primavera) se salta entera: una
//!   ocurrencia de las 02:30 en el día del cambio no ocurre, y la siguiente es
//!   la del día posterior;
//! - una hora local ambigua (vuelta al horario normal) ocurre una sola vez, con
//!   su instant más temprano.
//!
//! Sintaxis por campo: `*`, listas (`1,15`), rangos (`1-5`) y pasos
//! (`*/5`, `1-30/10`, `3/5` = `3-con-fin/5`). Sin nombres (`MAR`, `feb`): solo
//! números, para que el rechazo sea claro. El día de la semana es `0-7`, con
//! `7` normalizado a `0` (domingo); su rango natural `5-7` es fri-sat-sun.
//!
//! Regla vixie para días: si día del mes Y día de la semana están restringidos
//! (campo distinto de `*`), el día coincide si CUALQUIERO de los dos lo hace;
//! si solo uno está restringido, manda ese.

use chrono::{DateTime, Datelike, MappedLocalTime, NaiveDate, TimeZone};

/// Un fallo al validar una expresión cron.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cron inválido en el campo \"{field}\": {reason}")]
pub struct CronError {
    /// Campo donde se detectó el rechazo (`minuto`, `hora`, `día del mes`,
    /// `mes`, `día de la semana`), o `expresión` si falló la forma entera.
    pub field: &'static str,
    /// Motivo concreto del rechazo.
    pub reason: String,
}

/// Horizonte de búsqueda de la próxima ocurrencia: 4 años de días cubre el
/// 29 de febrero más lejano.
const HORIZONT_DIAS: i64 = 366 * 4 + 2;

/// Un cron ya parseado: máscaras de bits por campo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    /// Minutos que coinciden (bits 0..=59).
    minutes: u64,
    /// Horas que coinciden (bits 0..=23).
    hours: u32,
    /// Días del mes que coinciden (bits 1..=31).
    dom: u32,
    /// Meses que coinciden (bits 1..=12).
    months: u16,
    /// Días de la semana que coinciden (bits 0..=6; `7` se normaliza a 0).
    dow: u8,
    /// `true` si el campo día del mes no es `*`.
    dom_restricted: bool,
    /// `true` si el campo día de la semana no es `*`.
    dow_restricted: bool,
}

/// Parsea y valida una expresión cron de 5 campos.
pub fn parse(expr: &str) -> Result<CronSchedule, CronError> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(CronError {
            field: "expresión",
            reason: format!(
                "deben ser exactamente 5 campos minuto hora día_del_mes mes día_semana (lleva {})",
                fields.len()
            ),
        });
    }
    let (minutes, _) = parse_field(fields[0], 0, 59, "minuto", false)?;
    let (hours, _) = parse_field(fields[1], 0, 23, "hora", false)?;
    let (dom, dom_restricted) = parse_field(fields[2], 1, 31, "día del mes", false)?;
    let (months, _) = parse_field(fields[3], 1, 12, "mes", false)?;
    let (dow, dow_restricted) = parse_field(fields[4], 0, 7, "día de la semana", true)?;

    Ok(CronSchedule {
        minutes,
        hours: hours as u32,
        dom: dom as u32,
        months: months as u16,
        dow: dow as u8,
        dom_restricted,
        dow_restricted,
    })
}

/// Valida una expresión cron de 5 campos sin conservar el resultado.
pub fn validate(expr: &str) -> Result<(), CronError> {
    parse(expr).map(|_| ())
}

/// Valida que el texto sea una zona horaria IANA conocida
/// (`Europe/Madrid`, `UTC`, …).
pub fn validate_timezone(raw: &str) -> bool {
    raw.parse::<chrono_tz::Tz>().is_ok()
}

/// Parsea el texto de un campo: `*`, número, rango (`a-b`, ascendente) o paso
/// (`*/n`, `a-b/n`, `a/n` = `a-con-fin/n`). Devuelve la máscara de bits y si el
/// campo está restringido (solo un `*` exacto no lo está, regla vixie).
fn parse_field(
    field: &str,
    min: u32,
    max: u32,
    name: &'static str,
    dow: bool,
) -> Result<(u64, bool), CronError> {
    let err = |reason: String| CronError {
        field: name,
        reason,
    };
    let number = |text: &str| -> Result<u32, CronError> {
        text.parse::<u32>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| err(format!("\"{text}\" no es un número entre {min} y {max}")))
    };
    // `7` → `0`: el domingo solo tiene un bit en la máscara.
    let normalize = |n: u32| if dow && n == 7 { 0 } else { n };

    let mut mask: u64 = 0;
    for item in field.split(',') {
        let (range_text, step) = match item.split_once('/') {
            Some((prefix, step_text)) => {
                let step = step_text
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| err(format!("el paso \"{step_text}\" debe ser un número ≥ 1")))?;
                (prefix, step)
            }
            None => (item, 1),
        };

        if range_text == "*" {
            for n in min..=max {
                if (n - min).is_multiple_of(step) {
                    mask |= 1u64 << normalize(n);
                }
            }
            continue;
        }

        let (from, to) = match range_text.split_once('-') {
            Some((a, b)) => {
                let (from, to) = (number(a)?, number(b)?);
                if from > to {
                    return Err(err(format!("el rango {from}-{to} debe ser ascendente")));
                }
                (from, to)
            }
            // Un número con paso (`3/5`) es abreviatura de `3-con-fin/5`;
            // sin paso es exactamente ese valor único.
            None => {
                let n = number(range_text)?;
                (n, if step > 1 { max } else { n })
            }
        };
        for n in from..=to {
            if (n - from) % step == 0 {
                mask |= 1u64 << normalize(n);
            }
        }
    }
    if mask == 0 {
        return Err(err("la máscara quedó vacía (ningún valor coincide)".into()));
    }

    let star = field == "*";
    Ok((mask, !star))
}

impl CronSchedule {
    /// `true` si el día local coincide con los campos día del mes/mes/día de la
    /// semana (regla vixie: ambos restringidos → cualquiera coincide).
    pub fn day_matches(&self, day: NaiveDate) -> bool {
        let month_ok = (self.months & (1 << day.month())) != 0;
        if !month_ok {
            return false;
        }
        let dom_ok = self.dom_restricted && (self.dom & (1 << day.day())) != 0;
        let dow_ok = self
            .dow_restricted
            && (self.dow & (1 << day.weekday().num_days_from_sunday())) != 0;
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => dom_ok || dow_ok,
            (true, false) => dom_ok,
            (false, true) => dow_ok,
            (false, false) => true,
        }
    }

    /// La próxima ocurrencia estrictamente posterior a `from`, en la timezone
    /// de `from`. Devuelve `None` si en el horizonte (4 años) no hay ninguna.
    ///
    /// Horarios de verano: una hora local inexistente ([`MappedLocalTime::None`])
    /// se salta entera y una ambigua se toma con su instant más temprano.
    pub fn next_after<Tz: TimeZone>(&self, from: &DateTime<Tz>) -> Option<DateTime<Tz>> {
        let tz = from.timezone();
        // Los candidatos se recorren por día/hora/minuto coincidentes y se
        // filtran contra `from`: una ocurrencia anterior del mismo día no es
        // resultado (must be strictly after).
        let mut day: NaiveDate = from.date_naive();
        for _ in 0..HORIZONT_DIAS {
            if self.day_matches(day) {
                for hh in 0..24u32 {
                    if self.hours & (1 << hh) == 0 {
                        continue;
                    }
                    for mm in 0..60u32 {
                        if self.minutes & (1 << mm) == 0 {
                            continue;
                        }
                        let Some(naive) = day.and_hms_opt(hh, mm, 0) else {
                            continue;
                        };
                        match tz.from_local_datetime(&naive) {
                            MappedLocalTime::Single(dt) => {
                                if dt > *from {
                                    return Some(dt);
                                }
                            }
                            MappedLocalTime::Ambiguous(earliest, _) => {
                                if earliest > *from {
                                    return Some(earliest);
                                }
                            }
                            MappedLocalTime::None => {
                                // Hora local inexistente: se salta el minuto.
                                continue;
                            }
                        }
                    }
                }
            }
            day += chrono::Duration::days(1);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDateTime, Utc};
    use chrono_tz::{Europe::Madrid, Tz};

    /// Resuelve una hora local de Madrid a su instant UTC (la más temprana si
    /// es ambigua); `None` si la hora local no existe.
    fn local_to_utc(naive: chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
        match Madrid.from_local_datetime(&naive) {
            MappedLocalTime::Single(dt) => Some(dt.with_timezone(&Utc)),
            MappedLocalTime::Ambiguous(earliest, _) => Some(earliest.with_timezone(&Utc)),
            MappedLocalTime::None => None,
        }
    }

    /// Resuelve una hora local que se sabe univalua (falla el test si no existe
    /// o es ambigua: las pruebas evitan las horas del cambio).
    fn at_madrid(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> DateTime<Tz> {
        let naive =
            NaiveDateTime::parse_from_str(&format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:00"), "%Y-%m-%dT%H:%M:%S")
                .expect("fecha sintáctica");
        Madrid
            .from_local_datetime(&naive)
            .single()
            .unwrap_or_else(|| panic!("{y}-{m}-{d} {hh}:{mm} no es una hora univalua"))
    }

    /// Próxima ocurrencia de un cron con timezone UTC desde un instante UTC.
    fn next_utc(expr: &str, from: &str) -> String {
        let schedule = parse(expr).expect("cron válido");
        let from = from
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| panic!("instante sintáctico {from}"));
        schedule
            .next_after(&from)
            .unwrap_or_else(|| panic!("sin ocurrencia en el horizonte para {expr}"))
            .with_timezone(&Utc)
            .to_string()
    }

    /// Próxima ocurrencia de un cron en Madrid desde una hora local, como UTC.
    fn next_madrid(expr: &str, y: i32, m: u32, d: u32, hh: u32, mm: u32) -> String {
        let schedule = parse(expr).expect("cron válido");
        let from = at_madrid(y, m, d, hh, mm);
        schedule
            .next_after(&from)
            .expect("hay una ocurrencia siguiente")
            .with_timezone(&Utc)
            .to_string()
    }

    // ── cálculo próximo ────────────────────────────────────────────────

    #[test]
    fn next_after_devuelve_la_ocurrencia_estrictamente_posterior() {
        assert_eq!(next_utc("*/5 * * * *", "2026-03-04T09:03:00Z"), "2026-03-04 09:05:00 UTC");
        // mismo instante programado: estrictamente posterior
        assert_eq!(next_utc("*/5 * * * *", "2026-03-04T09:05:00Z"), "2026-03-04 09:10:00 UTC");
        assert_eq!(next_utc("0 9 * * *", "2026-03-04T09:00:00Z"), "2026-03-05 09:00:00 UTC");
        assert_eq!(next_utc("59 23 * * *", "2026-03-04T00:00:00Z"), "2026-03-04 23:59:00 UTC");
    }

    #[test]
    fn un_schedule_cada_5_minutos_avanza_prediciendo_varias_tandas() {
        let schedule = parse("*/5 * * * *").expect("válido");
        let from = at_madrid(2026, 3, 4, 9, 3);
        let series: Vec<String> = (0..3)
            .scan(from, |cursor, _| {
                let next = schedule.next_after(cursor).expect("siguiente");
                // El scan necesita el nuevo cursor tipado con la misma tz.
                *cursor = next;
                Some(next.with_timezone(&Utc).to_string())
            })
            .collect();
        assert_eq!(
            series,
            vec!["2026-03-04 08:05:00 UTC", "2026-03-04 08:10:00 UTC", "2026-03-04 08:15:00 UTC"]
        );
    }

    #[test]
    fn dia_semana_restringido_salta_al_laborable() {
        // 2026-03-08 es domingo: la ocurrencia de las 09:00 UTC ya pasó a las
        // 10:00, así que la siguiente es el domingo posterior 2026-03-15.
        assert_eq!(
            next_utc("0 9 * * 0", "2026-03-08T10:00:00Z"),
            "2026-03-15 09:00:00 UTC",
        );
        let schedule = parse("0 9 * * 1-5").expect("lunes-viernes");
        assert!(schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 6).expect("viernes")));
        assert!(!schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 7).expect("sábado")));
    }

    #[test]
    fn el_7_se_normaliza_a_domingo_y_cubre_el_caso_laborable() {
        let schedule = parse("0 9 * * 0,7").expect("domingo (dos formas)");
        assert!(schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 8).expect("domingo")));
    }

    #[test]
    fn feb_29_salta_los_anos_no_bisiestos() {
        // 2027 no es bisiesto → la primera ocurrencia es 2028-02-29.
        assert_eq!(
            next_utc("0 0 29 2 *", "2027-01-01T00:00:00Z"),
            "2028-02-29 00:00:00 UTC"
        );
    }

    // ── horarios de verano (Europe/Madrid) ─────────────────────────────

    #[test]
    fn salto_de_primavera_2026_se_salta_la_hora_local_inexistente() {
        // 2026-03-29: 02:00→03:00 local. La 02:30 de ese día no existe: la
        // siguiente ocurrencia del `30 2 * * *` es la del día posterior.
        let naive = NaiveDateTime::parse_from_str("2026-03-29T02:30:00", "%Y-%m-%dT%H:%M:%S").expect("sintáctica");
        assert!(local_to_utc(naive).is_none(), "2026-03-29 02:30 local no existe");
        assert_eq!(
            next_madrid("30 2 * * *", 2026, 3, 28, 12, 0),
            next_to_string("2026-03-30T02:30:00"),
        );

        // `*/5` avanza 01:55 → 03:00 local: la franja 02:00–02:55 no existe.
        // 03:00 CEST (UTC+2) = 01:00 UTC.
        assert_eq!(next_madrid("*/5 * * * *", 2026, 3, 29, 1, 56), "2026-03-29 01:00:00 UTC");
    }

    #[test]
    fn vuelta_al_horario_normal_2026_ocurre_una_vez_con_su_instant_mas_temprano() {
        // 2026-10-25: 02:30 local existe dos veces (00:30Z y 01:30Z). La
        // ocurrencia única de `30 2 * * *` es la más temprana.
        let naive = NaiveDateTime::parse_from_str("2026-10-25T02:30:00", "%Y-%m-%dT%H:%M:%S").expect("sintáctica");
        match Madrid.from_local_datetime(&naive) {
            MappedLocalTime::Ambiguous(_, _) => {}
            other => panic!("se esperaba ambigua, llegó {other:?}"),
        }
        assert_eq!(
            next_madrid("30 2 * * *", 2026, 10, 24, 12, 0),
            "2026-10-25 00:30:00 UTC"
        );
    }

    /// Siguiente hora local Y explotando el instante (helper de la prueba del
    /// salto que comparó contra una hora local justa).
    fn next_to_string(local: &str) -> String {
        let naive = NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M:%S").expect("sintáctica");
        next_to_string_inner(naive)
    }

    fn next_to_string_inner(naive: chrono::NaiveDateTime) -> String {
        local_to_utc(naive)
            .expect("hora local univalua")
            .to_string()
    }

    // ── regla vixie para días ──────────────────────────────────────────

    #[test]
    fn dia_del_mes_y_semana_ambos_restringidos_cualquiera_coincide() {
        // 2026-03-15 es domingo; 2026-03-16 es lunes.
        let schedule = parse("0 9 15 * 1").expect("dom-15 o lunes");
        assert!(schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 15).expect("domingo")));
        assert!(schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 16).expect("lunes")));
        assert!(!schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 17).expect("martes")));

        // Solo dom restringido → los lunes no coinciden.
        let schedule = parse("0 9 15 * *").expect("dom-15");
        assert!(!schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 16).expect("lunes")));

        // Solo dow restringido → el día 15 no manda.
        let schedule = parse("0 9 * * 1").expect("lunes");
        assert!(!schedule.day_matches(NaiveDate::from_ymd_opt(2026, 3, 15).expect("domingo")));
    }

    #[test]
    fn el_mes_restringido_ignora_los_dias_fuera() {
        assert_eq!(next_utc("0 9 1 12 *", "2026-06-15T00:00:00Z"), "2026-12-01 09:00:00 UTC");
        assert_eq!(next_utc("* * 1 1 *", "2026-06-15T00:00:00Z"), "2027-01-01 00:00:00 UTC");
    }

    // ── validez ────────────────────────────────────────────────────────

    #[test]
    fn un_schedule_malformado_se_rechaza_con_campo_y_motivo() {
        for (expr, campo) in [
            ("60 * * * *", "minuto"),
            ("* 24 * * *", "hora"),
            ("* * 32 * *", "día del mes"),
            ("* * * 13 *", "mes"),
            ("* * * * 8", "día de la semana"),
            ("a * * * *", "minuto"),
            ("5-1 * * * *", "minuto"),
            ("*/0 * * * *", "minuto"),
        ] {
            let err = validate(expr).expect_err(expr);
            assert_eq!(err.field, campo, "{expr}: {err}");
            assert!(err.to_string().starts_with("cron inválido en el campo"), "{err}");
        }
        for (expr, campo) in [
            ("0 9 * * * *", "expresión"),
            ("0 9 * *", "expresión"),
            ("", "expresión"),
            ("a * * * *", "minuto"),
        ] {
            let err = validate(expr).expect_err(expr);
            assert_eq!(err.field, campo, "{expr}: {err}");
        }
    }

    #[test]
    fn los_nombres_de_dia_y_mes_no_estan_soportados() {
        assert!(validate("0 9 * * 1,MON").is_err(), "nombres de día");
        assert!(validate("0 9 * * JAN").is_err(), "nombres de mes");
    }

    #[test]
    fn una_timezone_iana_desconocida_se_rechaza() {
        assert!(validate_timezone("Europe/Madrid"));
        assert!(validate_timezone("UTC"));
        assert!(validate_timezone("America/Argentina/Buenos_Aires"));
        assert!(!validate_timezone("Marte/Olympus"));
        assert!(!validate_timezone(""));
        assert!(!validate_timezone("Europe/"));
    }

    #[test]
    fn un_cron_correcto_pasa_la_validacion() {
        for expr in [
            "*/5 * * * *",
            "0 9 * * 1-5",
            "30 2 * * 7",      // dow 7 = domingo
            "0 9 15 * 1",      // dom + dow restringidos
            "1,15,30 * * * *", // lista
            "1-30/10 8-18 * * *",
            "0 9 29 2 *",      // feb 29
            "59 23 31 12 6",
        ] {
            validate(expr).unwrap_or_else(|err| panic!("{expr}: {err}"));
        }
    }
}
