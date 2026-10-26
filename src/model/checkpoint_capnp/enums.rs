use crate::checkpoint_capnp as wire;
use crate::model::checkpoint_request::{PreparedPathRole, StreamFormat};
use crate::model::working_log::CheckpointKind;

macro_rules! wire_enum {
    ($encode:ident, $decode:ident, $name:ident; $($variant:ident),+ $(,)?) => {
        pub(super) fn $encode(value: $name) -> wire::$name {
            match value {
                $($name::$variant => wire::$name::$variant,)+
            }
        }

        pub(super) fn $decode(value: wire::$name) -> $name {
            match value {
                $(wire::$name::$variant => $name::$variant,)+
            }
        }
    };
}

wire_enum!(encode_kind, decode_kind, CheckpointKind;
    Human, AiAgent, AiTab, KnownHuman,
);
wire_enum!(encode_role, decode_role, PreparedPathRole; Edited, WillEdit);
wire_enum!(encode_format, decode_format, StreamFormat;
    ClaudeJsonl, ContinueJson, GeminiJsonl, WindsurfJsonl, CodexJsonl, CursorJsonl,
    DroidJsonl, CopilotSessionJson, CopilotEventStreamJsonl, AmpThreadJson,
    OpenCodeSqlite, PiJsonl,
);
