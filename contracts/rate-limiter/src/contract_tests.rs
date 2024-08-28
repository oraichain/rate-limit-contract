#![cfg(test)]

use crate::packet::Packet;
use crate::{contract::*, test_msg_recv, test_msg_send, ContractError};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{from_json, Addr, Attribute, Uint128};

use crate::helpers::tests::verify_query_response;
use crate::msg::{ExecuteMsg, InstantiateMsg, PathMsg, QueryMsg, QuotaMsg};
use crate::state::tests::RESET_TIME_WEEKLY;
use crate::state::{RateLimit, RATE_LIMIT_TRACKERS};

const BRIDGE_CONTRACT: &str = "BRIDGE_CONTRACT";
const OWNER: &str = "Owner";

#[test] // Tests we ccan instantiate the contract and that the owners are set correctly
fn proper_instantiation() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg { paths: vec![] };
    let info = mock_info(OWNER, &vec![]);

    // we can just call .unwrap() to assert this was a success
    let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());
}

#[test] // Tests that when a packet is transferred, the peropper allowance is consummed
fn consume_allowance() {
    let mut deps = mock_dependencies();

    let quota = QuotaMsg::new(
        "weekly",
        RESET_TIME_WEEKLY,
        Uint128::new(1000000),
        Uint128::new(1000000),
    );
    let msg = InstantiateMsg {
        paths: vec![PathMsg {
            contract_addr: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel"),
            denom: format!("denom"),
            quotas: vec![quota],
        }],
    };
    let info = mock_info(OWNER, &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let info = mock_info(BRIDGE_CONTRACT, &vec![]);
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom") ,
        funds: Uint128::new(10000)
    );
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    println!("{:?}", res);
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "10000");

    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: Uint128::new(1000000)
    );
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert!(matches!(err, ContractError::RateLimitExceded { .. }));
}

#[test] // Tests that the balance of send and receive is maintained (i.e: recives are sustracted from the send allowance and sends from the receives)
fn symetric_flows_dont_consume_allowance() {
    let mut deps = mock_dependencies();

    let quota = QuotaMsg::new(
        "weekly",
        RESET_TIME_WEEKLY,
        Uint128::new(1000000),
        Uint128::new(1000000),
    );
    let msg = InstantiateMsg {
        paths: vec![PathMsg {
            contract_addr: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel"),
            denom: format!("denom"),
            quotas: vec![quota],
        }],
    };
    let info = mock_info(OWNER, &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let send_msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 300000_u32.into()
    );
    let recv_msg = test_msg_recv!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 300000_u32.into()
    );

    let info = mock_info(BRIDGE_CONTRACT, &vec![]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), send_msg.clone()).unwrap();
    let Attribute { key, value } = &res.attributes[3];
    assert_eq!(key, "weekly_used_in");
    assert_eq!(value, "0");
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "300000");

    let res = execute(deps.as_mut(), mock_env(), info.clone(), recv_msg.clone()).unwrap();
    let Attribute { key, value } = &res.attributes[3];
    assert_eq!(key, "weekly_used_in");
    assert_eq!(value, "0");
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "0");

    // We can still use the path. Even if we have sent more than the
    // allowance through the path (900 > 3000*.1), the current "balance"
    // of inflow vs outflow is still lower than the path's capacity/quota
    let res = execute(deps.as_mut(), mock_env(), info.clone(), recv_msg.clone()).unwrap();
    let Attribute { key, value } = &res.attributes[3];
    assert_eq!(key, "weekly_used_in");
    assert_eq!(value, "300000");
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "0");

    execute(deps.as_mut(), mock_env(), info.clone(), recv_msg.clone()).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), recv_msg.clone()).unwrap();

    let err = execute(deps.as_mut(), mock_env(), info.clone(), recv_msg.clone()).unwrap_err();

    assert!(matches!(err, ContractError::RateLimitExceded { .. }));
}

#[test] // Tests that we can have different quotas for send and receive.
fn asymetric_quotas() {
    let mut deps = mock_dependencies();

    let quota = QuotaMsg::new(
        "weekly",
        RESET_TIME_WEEKLY,
        Uint128::new(400000),
        Uint128::new(100000),
    );
    let msg = InstantiateMsg {
        paths: vec![PathMsg {
            contract_addr: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel"),
            denom: format!("denom"),
            quotas: vec![quota],
        }],
    };
    let info = mock_info(OWNER, &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    // Sending 50%
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 200000_u32.into()
    );
    let info = mock_info(BRIDGE_CONTRACT, &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "200000");

    // Sending 50% more. Allowed, as sending has a 100% allowance
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 200000_u32.into()
    );

    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "400000");

    // Receiving 1% should still work. 4% *sent* through the path, but we can still receive.
    let recv_msg = test_msg_recv!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 100000_u32.into()
    );
    let res = execute(deps.as_mut(), mock_env(), info.clone(), recv_msg).unwrap();
    let Attribute { key, value } = &res.attributes[3];
    assert_eq!(key, "weekly_used_in");
    assert_eq!(value, "0");
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "300000");

    // Sending 2%. Should fail. In balance, we've sent 4% and received 1%, so only 1% left to send.
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 200000_u32.into()
    );
    let err = execute(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap_err();
    assert!(matches!(err, ContractError::RateLimitExceded { .. }));

    // Sending 1%: Allowed; because sending has a 4% allowance. We've sent 4% already, but received 1%, so there's send cappacity again
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 100000_u32.into()
    );
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();
    let Attribute { key, value } = &res.attributes[3];
    assert_eq!(key, "weekly_used_in");
    assert_eq!(value, "0");
    let Attribute { key, value } = &res.attributes[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "400000");
}

#[test] // Tests we can get the current state of the trackers
fn query_state() {
    let mut deps = mock_dependencies();

    let quota = QuotaMsg::new(
        "weekly",
        RESET_TIME_WEEKLY,
        Uint128::new(1000000),
        Uint128::new(1000000),
    );
    let msg = InstantiateMsg {
        paths: vec![PathMsg {
            contract_addr: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel"),
            denom: format!("denom"),
            quotas: vec![quota],
        }],
    };
    let info = mock_info(OWNER, &vec![]);
    let env = mock_env();
    let _res = instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    let query_msg = QueryMsg::GetQuotas {
        contract: Addr::unchecked(BRIDGE_CONTRACT),
        channel_id: format!("channel"),
        denom: format!("denom"),
    };

    let res = query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap();
    let value: Vec<RateLimit> = from_json(&res).unwrap();
    assert_eq!(value[0].quota.name, "weekly");
    assert_eq!(value[0].quota.max_recv, Uint128::new(1000000));
    assert_eq!(value[0].quota.max_send, Uint128::new(1000000));
    assert_eq!(value[0].quota.duration, RESET_TIME_WEEKLY);
    assert_eq!(value[0].flow.inflow, Uint128::from(0_u32));
    assert_eq!(value[0].flow.outflow, Uint128::from(0_u32));
    assert_eq!(
        value[0].flow.period_end,
        env.block.time.plus_seconds(RESET_TIME_WEEKLY)
    );

    let info = mock_info(BRIDGE_CONTRACT, &[]);
    let send_msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 300_u32.into()
    );
    execute(deps.as_mut(), mock_env(), info.clone(), send_msg.clone()).unwrap();

    let recv_msg = test_msg_recv!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 30_u32.into()
    );
    execute(deps.as_mut(), mock_env(), info.clone(), recv_msg.clone()).unwrap();

    // Query
    let res = query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap();
    let value: Vec<RateLimit> = from_json(&res).unwrap();
    verify_query_response(
        &value[0],
        "weekly",
        Uint128::new(1000000),
        Uint128::new(1000000),
        RESET_TIME_WEEKLY,
        30_u32.into(),
        300_u32.into(),
        env.block.time.plus_seconds(RESET_TIME_WEEKLY),
    );
}

#[test] // Tests that undo reverts a packet send without affecting expiration or channel value
fn undo_send() {
    let mut deps = mock_dependencies();

    let quota = QuotaMsg::new(
        "weekly",
        RESET_TIME_WEEKLY,
        Uint128::new(1000000),
        Uint128::new(1000000),
    );
    let msg = InstantiateMsg {
        paths: vec![PathMsg {
            contract_addr: Addr::unchecked(BRIDGE_CONTRACT),
            channel_id: format!("channel"),
            denom: format!("denom"),
            quotas: vec![quota],
        }],
    };
    let info = mock_info(OWNER, &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let send_msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 300_u32.into()
    );
    let undo_msg = ExecuteMsg::UndoSend {
        packet: Packet::mock(format!("channel"), format!("denom"), 300_u32.into()),
    };
    let info = mock_info(BRIDGE_CONTRACT, &[]);

    execute(deps.as_mut(), mock_env(), info.clone(), send_msg.clone()).unwrap();

    let trackers = RATE_LIMIT_TRACKERS
        .load(
            &deps.storage,
            (
                Addr::unchecked(BRIDGE_CONTRACT),
                "channel".to_string(),
                "denom".to_string(),
            ),
        )
        .unwrap();
    assert_eq!(
        trackers.first().unwrap().flow.outflow,
        Uint128::from(300_u32)
    );
    let period_end = trackers.first().unwrap().flow.period_end;

    execute(deps.as_mut(), mock_env(), info.clone(), undo_msg.clone()).unwrap();

    let trackers = RATE_LIMIT_TRACKERS
        .load(
            &deps.storage,
            (
                Addr::unchecked(BRIDGE_CONTRACT),
                "channel".to_string(),
                "denom".to_string(),
            ),
        )
        .unwrap();
    assert_eq!(trackers.first().unwrap().flow.outflow, Uint128::from(0_u32));
    assert_eq!(trackers.first().unwrap().flow.period_end, period_end);
}
