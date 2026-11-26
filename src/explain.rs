//! Diagnostic codes, the explanation behind each, and closest-name suggestions.
//!
//! Every code an [`crate::Error`] can carry is declared here once, with the
//! version that introduced it and an explanation in `docs/errors/<code>.md`.
//! A code without its file does not compile.

/// What a diagnostic code means and how to fix it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Explanation {
    pub code: &'static str,
    pub summary: &'static str,
    /// Markdown: a failing specification, the error it gets, and a fix.
    pub text: &'static str,
}

impl Explanation {
    /// The explanation after its title and summary: the example and the fix.
    #[must_use]
    pub fn body(&self) -> &'static str {
        self.text
            .split_once("\n\n")
            .and_then(|(_, rest)| rest.split_once("\n\n"))
            .map_or(self.text, |(_, body)| body)
    }
}

macro_rules! codes {
    ($($variant:ident = $code:literal: $summary:literal;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Code {
            $($variant,)*
        }

        impl Code {
            pub(crate) fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)*
                }
            }
        }

        const EXPLANATIONS: &[Explanation] = &[
            $(Explanation {
                code: $code,
                summary: $summary,
                text: include_str!(concat!("../docs/errors/", $code, ".md")),
            },)*
        ];
    };
}

codes! {
    Spec = "spec": "The specification cannot be read: a syntax error or a rule of the language it breaks.";
    Usage = "usage": "The command line or a file it names cannot be used.";
    ModelFault = "model.fault": "Some reachable step leaves a declared range, divides by zero or reads a value that is not there.";
    Violation = "violation": "A property does not hold; the counterexample shows how.";
    Limit = "limit": "Exploration stopped at a limit before it could decide.";
    NameUnknown = "name.unknown": "A name that nothing declares.";
    TypeMismatch = "type.mismatch": "A value of one type where another is needed.";
    ComponentFieldConflict = "component.field_conflict": "Two system blocks declare the same state field.";
    ComponentDuplicateAction = "component.duplicate_action": "One system block declares an action twice.";
    ComponentForeignField = "component.foreign_field": "A system block touches a field that another block owns.";
    ComponentTypeConflict = "component.type_conflict": "Two system blocks declare a type under one name with different values.";
    ComponentParameterConflict = "component.parameter_conflict": "Two system blocks give a shared action's parameter different types.";
}

/// Every diagnostic code, in the order of `docs/errors/README.md`.
#[must_use]
pub fn explanations() -> &'static [Explanation] {
    EXPLANATIONS
}

/// The explanation of `code`, such as `name.unknown`.
#[must_use]
pub fn explanation(code: &str) -> Option<&'static Explanation> {
    EXPLANATIONS
        .iter()
        .find(|explanation| explanation.code == code)
}

const MAX_SUGGESTIONS: usize = 3;

/// The candidates closest to `target`: at edit distance at most 1 for names
/// of up to four characters and 2 for longer ones, counting a swap of two
/// neighbours as one edit and a difference in case alone as none. Ties are
/// all returned, in alphabetical order.
pub(crate) fn closest<'n>(
    target: &str,
    candidates: impl IntoIterator<Item = &'n str>,
) -> Vec<String> {
    let target: Vec<char> = target.chars().collect();
    let limit = if target.len() <= 4 { 1 } else { 2 };
    let mut best: Vec<(usize, &str)> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let chars: Vec<char> = candidate.chars().collect();
            if chars == target {
                return None;
            }
            let lower = |text: &[char]| {
                text.iter()
                    .flat_map(|c| c.to_lowercase())
                    .collect::<Vec<_>>()
            };
            let distance = if lower(&chars) == lower(&target) {
                0
            } else {
                distance(&target, &chars, limit)?
            };
            Some((distance, candidate))
        })
        .collect();
    let Some(nearest) = best.iter().map(|(distance, _)| *distance).min() else {
        return Vec::new();
    };
    best.retain(|(distance, _)| *distance == nearest);
    best.sort_unstable();
    best.dedup();
    best.into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, name)| name.to_owned())
        .collect()
}

/// The optimal string alignment distance from `a` to `b`, if it is at most `limit`.
fn distance(a: &[char], b: &[char], limit: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }
    let width = b.len() + 1;
    let mut rows = vec![0; (a.len() + 1) * width];
    for (j, cell) in rows.iter_mut().take(width).enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        rows[i * width] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut value = (rows[(i - 1) * width + j] + 1)
                .min(rows[i * width + j - 1] + 1)
                .min(rows[(i - 1) * width + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                value = value.min(rows[(i - 2) * width + j - 2] + 1);
            }
            rows[i * width + j] = value;
        }
    }
    let result = rows[a.len() * width + b.len()];
    (result <= limit).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::closest;

    #[test]
    fn suggests_the_nearest_names_within_the_limit() {
        let names = ["pending", "paid", "open", "closed", "Balance"];
        assert_eq!(closest("pendng", names), ["pending"]);
        assert_eq!(closest("pedning", names), ["pending"]);
        assert_eq!(closest("balance", names), ["Balance"]);
        assert_eq!(closest("opne", names), ["open"]);
        assert!(closest("shipped", names).is_empty());
        assert!(closest("pid", ["pad", "pod"]).len() == 2);
        assert_eq!(closest("pid", ["pod", "pad"]), ["pad", "pod"]);
        assert!(closest("ab", ["xy"]).is_empty());
        assert!(closest("open", names).is_empty());
    }
}
