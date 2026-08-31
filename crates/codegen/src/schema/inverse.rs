use std::{collections::BTreeMap, ptr::eq};

use anyhow::bail;
use proc_macro2::TokenStream;
use quote::quote;

use crate::{
    ast::{Assignment, AssignmentEffect, PredAll, Predicate, Scope, Transformation},
    schema::grammar::{Scopes, SettingInfo, generate_assignment, generate_predicate},
};

impl Transformation {
    pub fn is_invertible(&self, all: &[Self]) -> bool {
        let Some(Predicate::Equals(when_leaf)) = self.when.as_ref() else {
            return false;
        };
        if when_leaf.scope != Scope::Current {
            return false;
        }

        let Some(my_setters) = self.setters() else {
            return false;
        };

        for other in all {
            if eq(other, self) {
                continue;
            }
            let Some(other_setters) = other.setters() else {
                continue;
            };
            if pattern_matches(&my_setters, &other_setters) {
                return false;
            }
        }

        true
    }

    fn setters(&self) -> Option<Vec<(&str, &serde_json::Value)>> {
        self.apply
            .iter()
            .map(|a| match &a.effect {
                AssignmentEffect::Set(v) => Some((a.r#ref.as_str(), v)),
                AssignmentEffect::Clear => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InverseDecision {
    /// Invertible and not declared one-way: emit the inverse.
    Emit,
    /// Declared `one_way: true` in FML and indeed not invertible: skip quietly.
    DeclaredOneWay,
    /// Not invertible but not declared so: skip with a build warning so the
    /// author either fixes the transformation or declares the intent.
    UndeclaredOneWay,
}

impl Transformation {
    pub fn inverse_decision(&self, all: &[Self]) -> anyhow::Result<InverseDecision> {
        let invertible = self.is_invertible(all);
        Ok(match (self.one_way, invertible) {
            (false, true) => InverseDecision::Emit,
            (false, false) => InverseDecision::UndeclaredOneWay,
            (true, false) => InverseDecision::DeclaredOneWay,
            (true, true) => bail!(
                "transformation with when={:?} and apply={:?} is declared one_way but is invertible; drop `one_way: true` or make it non-invertible",
                self.when,
                self.apply
            ),
        })
    }
}

pub fn generate_inverses(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    transformations: &[Transformation],
    accessor: &TokenStream,
) -> anyhow::Result<TokenStream> {
    let mut blocks: Vec<TokenStream> = Vec::new();
    for t in transformations.iter().rev() {
        match t.inverse_decision(transformations)? {
            InverseDecision::Emit => blocks.push(generate_one_inverse(settings, t, accessor)?),
            InverseDecision::DeclaredOneWay => {}
            InverseDecision::UndeclaredOneWay => println!(
                "cargo:warning=skipping inverse for non-invertible transformation with when={:?} and apply={:?}; declare `one_way: true` in FML if this is intended.",
                t.when, t.apply
            ),
        }
    }
    Ok(quote! { #( #blocks )* })
}

fn generate_one_inverse(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    t: &Transformation,
    accessor: &TokenStream,
) -> anyhow::Result<TokenStream> {
    let set_leaves: Vec<Predicate> = t
        .apply
        .iter()
        .filter(|a| matches!(a.effect, AssignmentEffect::Set(_)))
        .map(Predicate::from)
        .collect();
    let match_pred = if set_leaves.len() == 1 {
        set_leaves
            .into_iter()
            .next()
            .expect("set_leaves has exactly one element")
    } else {
        PredAll { all: set_leaves }.into()
    };
    let condition = generate_predicate(settings, &match_pred, Scopes::new(accessor))?;

    let Some(Predicate::Equals(when_leaf)) = t.when.as_ref() else {
        bail!("generate_one_inverse called on non-alias transformation");
    };
    let when_assignment = Assignment {
        r#ref: when_leaf.r#ref.clone(),
        effect: AssignmentEffect::Set(when_leaf.equals.clone()),
    };
    let when_apply = generate_assignment(settings, &when_assignment, accessor)?;

    let mut clears = TokenStream::new();
    for a in &t.apply {
        if a.r#ref == when_leaf.r#ref {
            continue;
        }
        let info = settings.get(a.r#ref.as_str()).expect("ctx validated field");
        let ident = info.field_ident();
        clears.extend(quote! { #accessor.#ident = ::std::option::Option::None; });
    }

    Ok(quote! {
        if #condition {
            #when_apply
            #clears
        }
    })
}

fn pattern_matches(a: &[(&str, &serde_json::Value)], b: &[(&str, &serde_json::Value)]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .all(|(field, value)| b.iter().any(|(of, ov)| of == field && ov == value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Assignment, AssignmentEffect, LeafEquals, Scope};
    use serde_json::json;

    fn alias_t(
        trigger_field: &str,
        trigger_v: serde_json::Value,
        apply: Vec<(&str, serde_json::Value)>,
    ) -> Transformation {
        Transformation {
            one_way: false,
            when: Some(
                LeafEquals {
                    r#ref: trigger_field.into(),
                    scope: Scope::Current,
                    equals: trigger_v,
                }
                .into(),
            ),
            apply: apply
                .into_iter()
                .map(|(r, v)| Assignment {
                    r#ref: r.into(),
                    effect: AssignmentEffect::Set(v),
                })
                .collect(),
        }
    }

    #[test]
    fn multi_field_alias_is_invertible() {
        let t = alias_t(
            "dr",
            json!("hdr800_plus"),
            vec![("dr", json!("hdr800")), ("drp", json!("plus"))],
        );
        let all = vec![t];
        assert!(all[0].is_invertible(&all));
    }

    #[test]
    fn duplicate_pattern_blocks_invertibility() {
        let a = alias_t("x", json!("alpha"), vec![("p", json!(1)), ("q", json!(2))]);
        let b = alias_t("x", json!("beta"), vec![("p", json!(1)), ("q", json!(2))]);
        let all = vec![a, b];
        assert!(!all[0].is_invertible(&all));
        assert!(!all[1].is_invertible(&all));
    }

    #[test]
    fn original_scope_when_blocks_invertibility() {
        let t = Transformation {
            one_way: false,
            when: Some(
                LeafEquals {
                    r#ref: "dr".into(),
                    scope: Scope::Original,
                    equals: json!("hdr800_plus"),
                }
                .into(),
            ),
            apply: vec![Assignment {
                r#ref: "dr".into(),
                effect: AssignmentEffect::Set(json!("hdr800")),
            }],
        };
        let all = vec![t];
        assert!(!all[0].is_invertible(&all));
    }

    #[test]
    fn inverse_decision_distinguishes_declared_and_undeclared_one_way() {
        use super::InverseDecision;

        let mut invertible = alias_t("dr", json!("hdr800_plus"), vec![("dr", json!("hdr800"))]);
        let mut unconditional = Transformation {
            one_way: false,
            when: None,
            apply: vec![Assignment {
                r#ref: "head_0".into(),
                effect: AssignmentEffect::Set(json!(0)),
            }],
        };

        let all = vec![invertible.clone(), unconditional.clone()];
        assert_eq!(
            all[0].inverse_decision(&all).unwrap(),
            InverseDecision::Emit
        );
        assert_eq!(
            all[1].inverse_decision(&all).unwrap(),
            InverseDecision::UndeclaredOneWay
        );

        unconditional.one_way = true;
        let all = vec![invertible.clone(), unconditional];
        assert_eq!(
            all[1].inverse_decision(&all).unwrap(),
            InverseDecision::DeclaredOneWay
        );

        invertible.one_way = true;
        let all = vec![invertible];
        let error = all[0]
            .inverse_decision(&all)
            .expect_err("an invertible transformation must not be declared one_way");
        assert!(error.to_string().contains("one_way"), "{error}");
    }

    #[test]
    fn unconditional_transformation_not_invertible() {
        let t = Transformation {
            one_way: false,
            when: None,
            apply: vec![Assignment {
                r#ref: "head_0".into(),
                effect: AssignmentEffect::Set(json!(0)),
            }],
        };
        let all = vec![t];
        assert!(!all[0].is_invertible(&all));
    }
}
