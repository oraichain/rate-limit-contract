use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;

use cosmwasm_std::Uint128;

use crate::packet::Packet;

// PathMsg contains a channel_id and denom to represent a unique identifier within ibc-go, and a list of rate limit quotas
#[cw_serde]
pub struct PathMsg {
    pub contract_addr: Addr,
    pub channel_id: String,
    pub denom: String,
    pub quotas: Vec<QuotaMsg>,
}

impl PathMsg {
    pub fn new(
        contract_addr: &Addr,
        channel: impl Into<String>,
        denom: impl Into<String>,
        quotas: Vec<QuotaMsg>,
    ) -> Self {
        PathMsg {
            contract_addr: contract_addr.to_owned(),
            channel_id: channel.into(),
            denom: denom.into(),
            quotas,
        }
    }
}

// QuotaMsg represents a rate limiting Quota when sent as a wasm msg
#[cw_serde]
pub struct QuotaMsg {
    pub name: String,
    pub duration: u64,
    pub max_send: Uint128,
    pub max_receive: Uint128,
}

impl QuotaMsg {
    pub fn new(name: &str, seconds: u64, send: Uint128, recv: Uint128) -> Self {
        QuotaMsg {
            name: name.to_string(),
            duration: seconds,
            max_send: send,
            max_receive: recv,
        }
    }
}

/// Initialize the contract with the address of the IBC module and any existing channels.
/// Only the ibc module is allowed to execute actions on this contract
#[cw_serde]
pub struct InstantiateMsg {
    pub paths: Vec<PathMsg>,
}

/// The caller (IBC module) is responsible for correctly calculating the funds
/// being sent through the channel
#[cw_serde]
pub enum ExecuteMsg {
    AddPath {
        channel_id: String,
        denom: String,
        quotas: Vec<QuotaMsg>,
    },
    RemovePath {
        channel_id: String,
        denom: String,
    },
    ResetPathQuota {
        channel_id: String,
        denom: String,
        quota_id: String,
    },
    SendPacket {
        packet: Packet,
    },
    RecvPacket {
        packet: Packet,
    },
    UndoSend {
        packet: Packet,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Vec<crate::state::RateLimit>)]
    GetQuotas {
        contract: Addr,
        channel_id: String,
        denom: String,
    },
}

#[cw_serde]
pub enum MigrateMsg {}
