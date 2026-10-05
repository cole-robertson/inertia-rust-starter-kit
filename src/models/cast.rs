//! Active Model-style type casting for submitted form values, used by the models
//! `cargo loco generate scaffold` writes (`.loco-templates/`).
//!
//! A browser form submits strings (Inertia's `<Form>` posts them as JSON strings), so a
//! scaffolded `<Resource>Params` keeps every attribute as `Option<String>` ([`form_value`]
//! also accepts JSON numbers and booleans from API clients) and casts it with these
//! functions. Each takes the errors to add to, the attribute name and the raw value, and
//! reports Rails' messages: "can't be blank", "is not a number", "is not a date", and
//! `belongs_to`'s "must exist".

use std::str::FromStr;

use sea_orm::prelude::Date;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::users::Errors;

/// `#[serde(default, deserialize_with = "cast::form_value")]` for an `Option<String>`
/// attribute: a string as is, a number or boolean as its text, `null` as `None`.
///
/// # Errors
/// When the value is an array or an object.
pub fn form_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        Value::Number(n) => Ok(Some(n.to_string())),
        Value::Bool(b) => Ok(Some(b.to_string())),
        other => Err(serde::de::Error::custom(format!(
            "expected a string, number or boolean, got {other}"
        ))),
    }
}

fn blank(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim().is_empty())
}

/// A required string: stored as given; blank adds "can't be blank".
pub fn string(errors: &mut Errors, attribute: &str, value: Option<&str>) -> String {
    if blank(value) {
        errors.add(attribute, "can't be blank");
    }
    value.unwrap_or_default().to_owned()
}

/// An optional string: blank is `None`.
pub fn optional_string(_: &mut Errors, _: &str, value: Option<&str>) -> Option<String> {
    (!blank(value)).then(|| value.unwrap_or_default().to_owned())
}

/// A required number: blank adds "can't be blank", anything unparsable "is not a number".
pub fn number<T: FromStr + Default>(
    errors: &mut Errors,
    attribute: &str,
    value: Option<&str>,
) -> T {
    if blank(value) {
        errors.add(attribute, "can't be blank");
        return T::default();
    }
    optional_number(errors, attribute, value).unwrap_or_default()
}

/// An optional number: blank is `None`, anything unparsable adds "is not a number".
pub fn optional_number<T: FromStr>(
    errors: &mut Errors,
    attribute: &str,
    value: Option<&str>,
) -> Option<T> {
    let value = value.map(str::trim).filter(|v| !v.is_empty())?;
    let parsed = value.parse().ok();
    if parsed.is_none() {
        errors.add(attribute, "is not a number");
    }
    parsed
}

/// A checkbox: `1`, `true` and `on` are true; anything else, or no value, is false.
pub fn boolean(_: &mut Errors, _: &str, value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1" | "true" | "on"))
}

/// A required `YYYY-MM-DD` date (what `<input type="date">` submits).
pub fn date(errors: &mut Errors, attribute: &str, value: Option<&str>) -> Date {
    if blank(value) {
        errors.add(attribute, "can't be blank");
        return Date::default();
    }
    optional_date(errors, attribute, value).unwrap_or_default()
}

/// An optional `YYYY-MM-DD` date: blank is `None`, anything else unparsable adds
/// "is not a date".
pub fn optional_date(errors: &mut Errors, attribute: &str, value: Option<&str>) -> Option<Date> {
    let value = value.map(str::trim).filter(|v| !v.is_empty())?;
    let parsed = Date::parse_from_str(value, "%Y-%m-%d").ok();
    if parsed.is_none() {
        errors.add(attribute, "is not a date");
    }
    parsed
}

/// A required `belongs_to` foreign key (`project_id` for `belongs_to :project`): blank or not
/// an id adds "must exist" on `association` (`project`, not `project_id`), the way Rails 8
/// reports a missing parent. The caller checks that the row exists.
pub fn reference(errors: &mut Errors, association: &str, value: Option<&str>) -> i64 {
    if blank(value) {
        errors.add(association, "must exist");
        return 0;
    }
    optional_reference(errors, association, value).unwrap_or_default()
}

/// An optional foreign key (`belongs_to :project, optional: true`): blank is `None`, anything
/// that is not an id adds "must exist" on `association`.
pub fn optional_reference(
    errors: &mut Errors,
    association: &str,
    value: Option<&str>,
) -> Option<i64> {
    let value = value.map(str::trim).filter(|v| !v.is_empty())?;
    let id = value.parse().ok();
    if id.is_none() {
        errors.add(association, "must exist");
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(errors: &Errors, attribute: &str) -> Vec<String> {
        errors.get(attribute).unwrap_or_default().to_vec()
    }

    #[test]
    fn required_values_report_blank_including_whitespace() {
        let mut errors = Errors::new();
        string(&mut errors, "title", Some("  "));
        number::<i64>(&mut errors, "views", None);
        date(&mut errors, "published_on", Some(""));
        for attribute in ["title", "views", "published_on"] {
            assert_eq!(messages(&errors, attribute), ["can't be blank"]);
        }
    }

    #[test]
    fn optional_values_treat_blank_as_none_without_errors() {
        let mut errors = Errors::new();
        assert_eq!(optional_string(&mut errors, "body", Some(" ")), None);
        assert_eq!(optional_number::<f64>(&mut errors, "price", Some("")), None);
        assert_eq!(optional_date(&mut errors, "due_on", None), None);
        assert!(errors.is_empty());
    }

    #[test]
    fn values_are_cast_to_the_column_type() {
        let mut errors = Errors::new();
        assert_eq!(string(&mut errors, "title", Some(" Hi ")), " Hi ");
        assert_eq!(number::<i64>(&mut errors, "views", Some(" 42 ")), 42);
        assert_eq!(
            optional_number::<f64>(&mut errors, "price", Some("1.5")),
            Some(1.5)
        );
        assert_eq!(
            date(&mut errors, "published_on", Some("2026-01-02")),
            Date::from_ymd_opt(2026, 1, 2).unwrap()
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn unparsable_values_report_rails_messages() {
        let mut errors = Errors::new();
        number::<i64>(&mut errors, "views", Some("12abc"));
        optional_number::<i16>(&mut errors, "rank", Some("70000"));
        date(&mut errors, "published_on", Some("02/01/2026"));
        assert_eq!(messages(&errors, "views"), ["is not a number"]);
        assert_eq!(messages(&errors, "rank"), ["is not a number"]);
        assert_eq!(messages(&errors, "published_on"), ["is not a date"]);
    }

    #[test]
    fn a_missing_parent_is_reported_on_the_association_like_belongs_to() {
        let mut errors = Errors::new();
        reference(&mut errors, "project", Some(" "));
        reference(&mut errors, "owner", Some("abc"));
        assert_eq!(optional_reference(&mut errors, "parent", Some("x1")), None);
        assert_eq!(messages(&errors, "project"), ["must exist"]);
        assert_eq!(messages(&errors, "owner"), ["must exist"]);
        assert_eq!(messages(&errors, "parent"), ["must exist"]);
        assert_eq!(errors.get("project_id"), None, "keyed by association");

        let mut errors = Errors::new();
        assert_eq!(reference(&mut errors, "project", Some(" 7 ")), 7);
        assert_eq!(optional_reference(&mut errors, "parent", Some("")), None);
        assert!(errors.is_empty());
    }

    #[test]
    fn a_checkbox_is_true_only_for_checked_values() {
        let mut errors = Errors::new();
        for checked in ["1", "true", "on"] {
            assert!(boolean(&mut errors, "published", Some(checked)));
        }
        for unchecked in [None, Some(""), Some("0"), Some("false")] {
            assert!(!boolean(&mut errors, "published", unchecked));
        }
    }

    #[test]
    fn form_value_accepts_strings_numbers_booleans_and_null() {
        #[derive(Deserialize)]
        struct P {
            #[serde(default, deserialize_with = "form_value")]
            a: Option<String>,
            #[serde(default, deserialize_with = "form_value")]
            b: Option<String>,
            #[serde(default, deserialize_with = "form_value")]
            c: Option<String>,
            #[serde(default, deserialize_with = "form_value")]
            d: Option<String>,
            #[serde(default, deserialize_with = "form_value")]
            missing: Option<String>,
        }
        let p: P =
            serde_json::from_value(serde_json::json!({"a": "x", "b": 7, "c": true, "d": null}))
                .unwrap();
        assert_eq!(
            [p.a, p.b, p.c, p.d, p.missing],
            [
                Some("x".into()),
                Some("7".into()),
                Some("true".into()),
                None,
                None
            ]
        );
        assert!(serde_json::from_value::<P>(serde_json::json!({"a": [1]})).is_err());
    }
}
