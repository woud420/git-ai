use git_ai::model::checkpoint_delivery::CheckpointDelivery;
use git_ai::model::repository::checkpoint_outbox;

#[derive(Clone, Copy)]
pub(super) enum Codec {
    Json,
    OutboxCbor,
}

impl Codec {
    pub(super) const ALL: [Self; 2] = [Self::Json, Self::OutboxCbor];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::OutboxCbor => "existing_outbox_cbor",
        }
    }

    pub(super) fn encode(self, delivery: &CheckpointDelivery) -> Vec<u8> {
        match self {
            Self::Json => {
                delivery.validate().unwrap();
                serde_json::to_vec(delivery).unwrap()
            }
            Self::OutboxCbor => checkpoint_outbox::encode_delivery(delivery).unwrap(),
        }
    }

    pub(super) fn decode(self, bytes: &[u8]) -> CheckpointDelivery {
        match self {
            Self::Json => {
                let delivery: CheckpointDelivery = serde_json::from_slice(bytes).unwrap();
                delivery.validate().unwrap();
                delivery
            }
            Self::OutboxCbor => checkpoint_outbox::decode_delivery(bytes).unwrap(),
        }
    }
}
