use cosmwasm_std::{to_json_binary, Addr, Binary, Deps, StdResult};

use crate::state::{Path, RATE_LIMIT_TRACKERS};

pub fn get_quotas(
    deps: Deps,
    contract: Addr,
    channel_id: impl Into<String>,
    denom: impl Into<String>,
) -> StdResult<Binary> {
    let path = Path::new(&contract, channel_id, denom);
    to_json_binary(&RATE_LIMIT_TRACKERS.load(deps.storage, path.into())?)
}
