use crate::msg::{PathMsg, QuotaMsg};
use crate::packet::Packet;
use crate::state::{Flow, FlowType, Path, RateLimit, RATE_LIMIT_TRACKERS};
use crate::ContractError;
use cosmwasm_std::{Addr, DepsMut, Response, Timestamp, Uint128};

pub fn add_new_paths(
    deps: DepsMut,
    path_msgs: Vec<PathMsg>,
    now: Timestamp,
) -> Result<(), ContractError> {
    for path_msg in path_msgs {
        let path = Path::new(&path_msg.contract_addr, path_msg.channel_id, path_msg.denom);

        RATE_LIMIT_TRACKERS.save(
            deps.storage,
            path.into(),
            &path_msg
                .quotas
                .iter()
                .map(|q| RateLimit {
                    quota: q.into(),
                    flow: Flow::new(0_u128, 0_u128, now, q.duration),
                })
                .collect(),
        )?
    }
    Ok(())
}

pub fn try_add_path(
    deps: DepsMut,
    contract: Addr,
    channel_id: String,
    denom: String,
    quotas: Vec<QuotaMsg>,
    now: Timestamp,
) -> Result<Response, ContractError> {
    // codenit: should we make a function for checking this authorization?

    add_new_paths(
        deps,
        vec![PathMsg::new(&contract, &channel_id, &denom, quotas)],
        now,
    )?;

    Ok(Response::new()
        .add_attribute("method", "try_add_channel")
        .add_attribute("contract", contract.as_str())
        .add_attribute("channel_id", channel_id)
        .add_attribute("denom", denom))
}

pub fn try_remove_path(
    deps: DepsMut,
    contract: Addr,
    channel_id: String,
    denom: String,
) -> Result<Response, ContractError> {
    let path = Path::new(&contract, &channel_id, &denom);
    RATE_LIMIT_TRACKERS.remove(deps.storage, path.into());
    Ok(Response::new()
        .add_attribute("method", "try_remove_channel")
        .add_attribute("contract", contract.as_str())
        .add_attribute("denom", denom)
        .add_attribute("channel_id", channel_id))
}

// Reset specified quote_id for the given channel_id
pub fn try_reset_path_quota(
    deps: DepsMut,
    contract: Addr,
    channel_id: String,
    denom: String,
    quota_id: String,
    now: Timestamp,
) -> Result<Response, ContractError> {
    let path = Path::new(&contract, &channel_id, &denom);
    RATE_LIMIT_TRACKERS.update(deps.storage, path.into(), |maybe_rate_limit| {
        match maybe_rate_limit {
            None => Err(ContractError::QuotaNotFound {
                quota_id,
                channel_id: channel_id.clone(),
                denom: denom.clone(),
            }),
            Some(mut limits) => {
                // Q: What happens here if quote_id not found? seems like we return ok?
                limits.iter_mut().for_each(|limit| {
                    if limit.quota.name == quota_id.as_ref() {
                        limit.flow.expire(now, limit.quota.duration)
                    }
                });
                Ok(limits)
            }
        }
    })?;

    Ok(Response::new()
        .add_attribute("method", "try_reset_channel")
        .add_attribute("contract", contract.as_str())
        .add_attribute("denom", denom)
        .add_attribute("channel_id", channel_id))
}

// This function will process a packet and extract the paths information, funds,
// and channel value from it. This is will have to interact with the chain via grpc queries to properly
// obtain this information.
//
// For backwards compatibility, we're teporarily letting the chain override the
// denom and channel value, but these should go away in favour of the contract
// extracting these from the packet
pub fn process_packet(
    deps: DepsMut,
    contract: Addr,
    packet: Packet,
    direction: FlowType,
    now: Timestamp,
) -> Result<Response, ContractError> {
    let path = &Path::new(&contract, &packet.channel, &packet.denom);
    let funds = packet.amount;

    try_transfer(deps, path, funds, direction, now)
}

/// This function checks the rate limit and, if successful, stores the updated data about the value
/// that has been transfered through the channel for a specific denom.
/// If the period for a RateLimit has ended, the Flow information is reset.
///
/// The channel_value is the current value of the denom for the the channel as
/// calculated by the caller. This should be the total supply of a denom
pub fn try_transfer(
    deps: DepsMut,
    path: &Path,
    funds: Uint128,
    direction: FlowType,
    now: Timestamp,
) -> Result<Response, ContractError> {
    // Fetch trackers for the requested path
    let mut trackers = RATE_LIMIT_TRACKERS
        .may_load(deps.storage, path.into())?
        .unwrap_or_default();

    let not_configured = trackers.is_empty();

    if not_configured {
        // No Quota configured for the current path. Allowing all messages.
        return Ok(Response::new()
            .add_attribute("method", "try_transfer")
            .add_attribute("contract", path.contract.as_str())
            .add_attribute("channel_id", path.channel.to_string())
            .add_attribute("denom", path.denom.to_string())
            .add_attribute("quota", "none"));
    }

    // If any of the RateLimits fails, allow_transfer() will return
    // ContractError::RateLimitExceded, which we'll propagate out
    let results: Vec<RateLimit> = trackers
        .iter_mut()
        .map(|limit| limit.allow_transfer(path, &direction, funds, now))
        .collect::<Result<_, ContractError>>()?;

    RATE_LIMIT_TRACKERS.save(deps.storage, path.into(), &results)?;

    let response = Response::new()
        .add_attribute("method", "try_transfer")
        .add_attribute("channel_id", path.channel.to_string())
        .add_attribute("denom", path.denom.to_string());

    // Adds the attributes for each path to the response. In prod, the
    // addtribute add_rate_limit_attributes is a noop
    // let response: Result<Response, ContractError> =
    //     results.iter().fold(Ok(response), |acc, result| {
    //         Ok(add_rate_limit_attributes(acc?, result))
    //     });
    results.iter().fold(Ok(response), |acc, result| {
        Ok(add_rate_limit_attributes(acc?, result))
    })
}

// #[cfg(any(feature = "verbose_responses", test))]
fn add_rate_limit_attributes(response: Response, result: &RateLimit) -> Response {
    let (used_in, used_out) = result.flow.balance();
    let (max_in, max_out) = result.quota.capacity();
    // These attributes are only added during testing. That way we avoid
    // calculating these again on prod.
    response
        .add_attribute(
            format!("{}_used_in", result.quota.name),
            used_in.to_string(),
        )
        .add_attribute(
            format!("{}_used_out", result.quota.name),
            used_out.to_string(),
        )
        .add_attribute(format!("{}_max_in", result.quota.name), max_in.to_string())
        .add_attribute(
            format!("{}_max_out", result.quota.name),
            max_out.to_string(),
        )
        .add_attribute(
            format!("{}_period_end", result.quota.name),
            result.flow.period_end.to_string(),
        )
}

// This function manually injects an inflow. This is used when reverting a
// packet that failed ack or timed-out.
pub fn undo_send(deps: DepsMut, contract: Addr, packet: Packet) -> Result<Response, ContractError> {
    let path = &Path::new(&contract, packet.channel, packet.denom);
    let funds = packet.amount;

    let mut trackers = RATE_LIMIT_TRACKERS
        .may_load(deps.storage, path.into())?
        .unwrap_or_default();

    let not_configured = trackers.is_empty();

    if not_configured {
        // No Quota configured for the current path. Allowing all messages.
        return Ok(Response::new()
            .add_attribute("method", "try_transfer")
            .add_attribute("contract", contract.as_str())
            .add_attribute("channel_id", path.channel.to_string())
            .add_attribute("denom", path.denom.to_string())
            .add_attribute("quota", "none"));
    }

    // We force update the flow to remove a failed send
    let results: Vec<RateLimit> = trackers
        .iter_mut()
        .map(|limit| {
            limit.flow.undo_flow(FlowType::Out, funds);
            limit.to_owned()
        })
        .collect();

    RATE_LIMIT_TRACKERS.save(deps.storage, path.into(), &results)?;

    Ok(Response::new()
        .add_attribute("method", "undo_send")
        .add_attribute("contract", contract.as_str())
        .add_attribute("channel_id", path.channel.to_string())
        .add_attribute("denom", path.denom.to_string()))
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{from_json, Addr, StdError, Uint128};

    use crate::contract::{execute, query};
    use crate::helpers::tests::verify_query_response;
    use crate::msg::{ExecuteMsg, QueryMsg, QuotaMsg};
    use crate::state::RateLimit;

    const BRIDGE_CONTRACT: &str = "bridge_contract";

    #[test] // Tests AddPath and RemovePath messages
    fn management_add_and_remove_path() {
        let mut deps = mock_dependencies();

        let msg = ExecuteMsg::AddPath {
            channel_id: format!("channel"),
            denom: format!("denom"),
            quotas: vec![QuotaMsg {
                name: "daily".to_string(),
                duration: 1600,
                max_send: Uint128::new(1000000),
                max_receive: Uint128::new(1000000),
            }],
        };
        let info = mock_info(BRIDGE_CONTRACT, &vec![]);

        let env = mock_env();
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        let query_msg = QueryMsg::GetQuotas {
            contract: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel"),
            denom: format!("denom"),
        };

        let res = query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap();

        let value: Vec<RateLimit> = from_json(&res).unwrap();
        verify_query_response(
            &value[0],
            "daily",
            Uint128::new(1000000),
            Uint128::new(1000000),
            1600,
            0_u32.into(),
            0_u32.into(),
            env.block.time.plus_seconds(1600),
        );

        assert_eq!(value.len(), 1);

        // Add another path
        let msg = ExecuteMsg::AddPath {
            channel_id: format!("channel2"),
            denom: format!("denom"),
            quotas: vec![QuotaMsg {
                name: "daily".to_string(),
                duration: 1600,
                max_send: Uint128::new(1000000),
                max_receive: Uint128::new(1000000),
            }],
        };
        let info = mock_info(BRIDGE_CONTRACT, &vec![]);

        let env = mock_env();
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // remove the first one
        let msg = ExecuteMsg::RemovePath {
            channel_id: format!("channel"),
            denom: format!("denom"),
        };

        let info = mock_info(BRIDGE_CONTRACT, &vec![]);
        let env = mock_env();
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // The channel is not there anymore
        let err = query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap_err();
        assert!(matches!(err, StdError::NotFound { .. }));

        // The second channel is still there
        let query_msg = QueryMsg::GetQuotas {
            contract: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel2"),
            denom: format!("denom"),
        };
        let res = query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap();
        let value: Vec<RateLimit> = from_json(&res).unwrap();
        assert_eq!(value.len(), 1);
        verify_query_response(
            &value[0],
            "daily",
            Uint128::new(1000000),
            Uint128::new(1000000),
            1600,
            0_u32.into(),
            0_u32.into(),
            env.block.time.plus_seconds(1600),
        );

        // Paths are overriden if they share a name and denom
        let msg = ExecuteMsg::AddPath {
            channel_id: format!("channel2"),
            denom: format!("denom"),
            quotas: vec![QuotaMsg {
                name: "different".to_string(),
                duration: 5000,
                max_send: Uint128::new(10000000),
                max_receive: Uint128::new(10000000),
            }],
        };
        let info = mock_info(BRIDGE_CONTRACT, &vec![]);

        let env = mock_env();
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let query_msg = QueryMsg::GetQuotas {
            contract: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel2"),
            denom: format!("denom"),
        };
        let res = query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap();
        let value: Vec<RateLimit> = from_json(&res).unwrap();
        assert_eq!(value.len(), 1);

        verify_query_response(
            &value[0],
            "different",
            Uint128::new(10000000),
            Uint128::new(10000000),
            5000,
            0_u32.into(),
            0_u32.into(),
            env.block.time.plus_seconds(5000),
        );
    }
}
