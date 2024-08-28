use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

// An IBC packet
#[cw_serde]
pub struct Packet {
    pub channel: String,
    pub denom: String,
    pub amount: Uint128,
}

// Helpers

impl Packet {
    pub fn mock(channel: String, denom: String, amount: Uint128) -> Self {
        Packet {
            channel,
            denom,
            amount,
        }
    }
}

// Create a new packet for testing
#[cfg(test)]
#[macro_export]
macro_rules! test_msg_send {
    (channel_id: $channel_id:expr, denom: $denom:expr, funds: $funds:expr) => {
        $crate::msg::ExecuteMsg::SendPacket {
            packet: $crate::packet::Packet::mock($channel_id, $denom, $funds),
        }
    };
}

#[cfg(test)]
#[macro_export]
macro_rules! test_msg_recv {
    (channel_id: $channel_id:expr, denom: $denom:expr, funds: $funds:expr) => {
        $crate::msg::ExecuteMsg::RecvPacket {
            packet: $crate::packet::Packet::mock($channel_id, $denom, $funds),
        }
    };
}
