//! Ошибки ядра. Правило: каждое сообщение говорит, *что* не так и *что сделать*.

use crate::object::GameObjectId;

#[derive(Debug, thiserror::Error)]
pub enum ReflectError {
    #[error("component `{component}` has no field `{field}` (available fields: {available})")]
    NoSuchField {
        component: &'static str,
        field: String,
        available: String,
    },
    #[error("field `{component}.{field}` expects {expected}, but got {got}")]
    TypeMismatch {
        component: &'static str,
        field: &'static str,
        expected: &'static str,
        got: &'static str,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error(
        "unknown component type `{kind}`.{hint} \
         Register it before loading: `app.register::<{kind}>()`"
    )]
    UnknownComponent { kind: String, hint: String },

    #[error("in object `{object}`: {source}")]
    InObject {
        object: String,
        #[source]
        source: Box<SceneError>,
    },

    #[error(transparent)]
    Reflect(#[from] ReflectError),

    #[error("cannot access `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot parse `{path}`: {message}")]
    Parse { path: String, message: String },

    #[error("prefab `{reference}` not found: {reason}")]
    PrefabNotFound { reference: String, reason: String },

    #[error("prefab `{reference}` is nested too deep — does it (indirectly) include itself?")]
    PrefabCycle { reference: String },

    #[error(
        "override in `{object}` targets `{target}`, which does not exist in prefab `{prefab}`.{hint}"
    )]
    BadOverride {
        object: String,
        prefab: String,
        target: String,
        hint: String,
    },

    #[error("game object {0:?} does not exist (destroyed or never spawned)")]
    DeadObject(GameObjectId),

    #[error("cannot make `{child}` a child of `{parent}`: that would create a cycle in the hierarchy")]
    Cycle { child: String, parent: String },
}

impl SceneError {
    pub(crate) fn in_object(self, object: &str) -> Self {
        SceneError::InObject {
            object: object.to_string(),
            source: Box::new(self),
        }
    }
}

/// Подсказка «возможно, вы имели в виду …» по расстоянию Левенштейна.
pub fn did_you_mean<'a>(needle: &str, candidates: impl IntoIterator<Item = &'a str>) -> String {
    let best = candidates
        .into_iter()
        .map(|c| (levenshtein(needle, c), c))
        .min_by_key(|(d, _)| *d);
    match best {
        Some((d, c)) if d <= needle.len().max(3) / 2 => format!(" Did you mean `{c}`?"),
        _ => String::new(),
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().flat_map(char::to_lowercase).collect();
    let b: Vec<char> = b.chars().flat_map(char::to_lowercase).collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { prev } else { prev + 1 };
            prev = row[j + 1];
            row[j + 1] = cost.min(row[j] + 1).min(prev + 1);
        }
    }
    row[b.len()]
}
