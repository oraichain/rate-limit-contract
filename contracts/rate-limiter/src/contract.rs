#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::state::FlowType;
use crate::{execute, query};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:rate-limiter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    execute::add_new_paths(deps, msg.paths, env.block.time)?;

    Ok(Response::new().add_attribute("method", "instantiate"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddPath {
            channel_id,
            denom,
            quotas,
        } => execute::try_add_path(deps, info.sender, channel_id, denom, quotas, env.block.time),
        ExecuteMsg::RemovePath { channel_id, denom } => {
            execute::try_remove_path(deps, info.sender, channel_id, denom)
        }
        ExecuteMsg::ResetPathQuota {
            channel_id,
            denom,
            quota_id,
        } => execute::try_reset_path_quota(
            deps,
            info.sender,
            channel_id,
            denom,
            quota_id,
            env.block.time,
        ),
        ExecuteMsg::SendPacket { packet } => {
            execute::process_packet(deps, info.sender, packet, FlowType::Out, env.block.time)
        }
        ExecuteMsg::RecvPacket { packet } => {
            execute::process_packet(deps, info.sender, packet, FlowType::In, env.block.time)
        }
        ExecuteMsg::UndoSend { packet } => execute::undo_send(deps, info.sender, packet),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetQuotas {
            contract,
            channel_id,
            denom,
        } => query::get_quotas(deps, contract, channel_id, denom),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    unimplemented!()
}
