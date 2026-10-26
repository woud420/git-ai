@0xc91d912f4a5d35b3;

# The live checkpoint transport only. Durable outbox records remain CBOR.
# Field and enum ordinals are part of the protocol and must never be reused.

struct CheckpointDelivery {
  schemaVersion @0 :UInt16;
  deliveryId @1 :Text;
  batchId @2 :Text;
  batchOrdinal @3 :UInt32;
  capturedAtUnixMs @4 :UInt64;
  producerVersion @5 :Text;
  request @6 :CheckpointRequest;
}

struct CheckpointRequest {
  traceId @0 :Text;
  checkpointKind @1 :CheckpointKind;
  agentId @2 :AgentId;
  files @3 :List(CheckpointFile);
  pathRole @4 :PreparedPathRole;
  streamSource @5 :StreamSource;
  metadata @6 :List(MetadataEntry);
  deliveryId @7 :OptionalText;
}

struct CheckpointFile {
  path @0 :Text;
  content @1 :OptionalText;
  repoWorkDir @2 :Text;
  baseCommit @3 :BaseCommit;
}

struct BaseCommit {
  union {
    initial @0 :Void;
    sha @1 :Text;
  }
}

struct OptionalText {
  union {
    absent @0 :Void;
    value @1 :Text;
  }
}

struct AgentId {
  tool @0 :Text;
  id @1 :Text;
  model @2 :Text;
}

struct StreamSource {
  path @0 :Text;
  format @1 :StreamFormat;
  sessionId @2 :Text;
  externalSessionId @3 :Text;
  externalParentSessionId @4 :OptionalText;
}

struct MetadataEntry {
  key @0 :Text;
  value @1 :Text;
}

enum CheckpointKind {
  human @0;
  aiAgent @1;
  aiTab @2;
  knownHuman @3;
}

enum PreparedPathRole {
  edited @0;
  willEdit @1;
}

enum StreamFormat {
  claudeJsonl @0;
  continueJson @1;
  geminiJsonl @2;
  windsurfJsonl @3;
  codexJsonl @4;
  cursorJsonl @5;
  droidJsonl @6;
  copilotSessionJson @7;
  copilotEventStreamJsonl @8;
  ampThreadJson @9;
  openCodeSqlite @10;
  piJsonl @11;
}
