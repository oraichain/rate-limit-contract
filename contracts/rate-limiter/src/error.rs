use cosmwasm_std::{StdError, Timestamp, Uint128};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("IBC Rate Limit exceeded for {contract}{channel}/{denom}. Tried to transfer {amount} which exceeds capacity on the '{quota_name}' quota ({used}/{max}). Try again after {reset:?}")]
    RateLimitExceded {
        contract: String,
        channel: String,
        denom: String,
        amount: Uint128,
        quota_name: String,
        used: Uint128,
        max: Uint128,
        reset: Timestamp,
    },

    #[error("Quota {quota_id} not found for channel {channel_id}")]
    QuotaNotFound {
        quota_id: String,
        channel_id: String,
        denom: String,
    },
}
