#![cfg(test)]
use crate::{helpers::RateLimitingContract, test_msg_send, ContractError};
use cosmwasm_std::{Addr, Coin, Empty, Timestamp, Uint128};
use cosmwasm_testing_util::{App, AppBuilder, Contract, ContractWrapper, Executor};

use crate::{
    msg::{InstantiateMsg, PathMsg, QuotaMsg},
    state::tests::{RESET_TIME_DAILY, RESET_TIME_MONTHLY, RESET_TIME_WEEKLY},
};

pub fn contract_template() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        crate::contract::execute,
        crate::contract::instantiate,
        crate::contract::query,
    );

    Box::new(contract)
}

const USER: &str = "USER";
const OWNER: &str = "OWNER";
const BRIDGE_CONTRACT: &str = "BRIDGE_CONTRACT";
const NATIVE_DENOM: &str = "orai";

fn mock_app() -> App {
    AppBuilder::new().build(|router, _, storage| {
        router
            .bank
            .init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![Coin {
                    denom: NATIVE_DENOM.to_string(),
                    amount: Uint128::new(1_000_000),
                }],
            )
            .unwrap();
    })
}

// Instantiate the contract
fn proper_instantiate(paths: Vec<PathMsg>) -> (App, RateLimitingContract) {
    let mut app = mock_app();
    let cw_code_id = app.store_code(contract_template());

    let msg = InstantiateMsg { paths };

    let cw_rate_limit_contract_addr = app
        .instantiate_contract(cw_code_id, Addr::unchecked(OWNER), &msg, &[], "test", None)
        .unwrap();

    let cw_rate_limit_contract = RateLimitingContract(cw_rate_limit_contract_addr);

    (app, cw_rate_limit_contract)
}

use cosmwasm_std::Attribute;

#[test] // Checks that the RateLimit flows are expired properly when time passes
fn expiration() {
    let quota = QuotaMsg::new(
        "weekly",
        RESET_TIME_WEEKLY,
        Uint128::new(1000),
        Uint128::new(1000),
    );

    let (mut app, cw_rate_limit_contract) = proper_instantiate(vec![PathMsg {
        contract_addr: Addr::unchecked(BRIDGE_CONTRACT),
        channel_id: format!("channel"),
        denom: format!("denom"),
        quotas: vec![quota],
    }]);

    // Using all the allowance
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 300_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    let res = app
        .execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap();

    let Attribute { key, value } = &res.custom_attrs(1)[3];
    assert_eq!(key, "weekly_used_in");
    assert_eq!(value, "0");
    let Attribute { key, value } = &res.custom_attrs(1)[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "300");
    let Attribute { key, value } = &res.custom_attrs(1)[5];
    assert_eq!(key, "weekly_max_in");
    assert_eq!(value, "1000");
    let Attribute { key, value } = &res.custom_attrs(1)[6];
    assert_eq!(key, "weekly_max_out");
    assert_eq!(value, "1000");

    // Another packet is rate limited
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 800_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    let err = app
        .execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap_err();

    assert_eq!(
        err.downcast_ref::<ContractError>().unwrap(),
        &ContractError::RateLimitExceded {
            contract: BRIDGE_CONTRACT.to_string(),
            channel: "channel".to_string(),
            denom: "denom".to_string(),
            amount: Uint128::new(800),
            quota_name: "weekly".to_string(),
            used: Uint128::new(300),
            max: Uint128::new(1000),
            reset: Timestamp::from_nanos(1572402219879305533),
        }
    );

    // ... Time passes
    app.update_block(|b| {
        b.height += 1000;
        b.time = b.time.plus_seconds(RESET_TIME_WEEKLY + 1)
    });

    // Sending the packet should work now
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 800_u32.into()
    );

    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    let res = app
        .execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap();

    let Attribute { key, value } = &res.custom_attrs(1)[3];
    assert_eq!(key, "weekly_used_in");
    assert_eq!(value, "0");
    let Attribute { key, value } = &res.custom_attrs(1)[4];
    assert_eq!(key, "weekly_used_out");
    assert_eq!(value, "800");
    let Attribute { key, value } = &res.custom_attrs(1)[5];
    assert_eq!(key, "weekly_max_in");
    assert_eq!(value, "1000");
    let Attribute { key, value } = &res.custom_attrs(1)[6];
    assert_eq!(key, "weekly_max_out");
    assert_eq!(value, "1000");
}

#[test] // Tests we can have different maximums for different quotaas (daily, weekly, etc) and that they all are active at the same time
fn multiple_quotas() {
    let quotas = vec![
        QuotaMsg::new(
            "daily",
            RESET_TIME_DAILY,
            Uint128::new(1000),
            Uint128::new(1000),
        ),
        QuotaMsg::new(
            "weekly",
            RESET_TIME_WEEKLY,
            Uint128::new(5000),
            Uint128::new(5000),
        ),
        QuotaMsg::new(
            "monthly",
            RESET_TIME_MONTHLY,
            Uint128::new(5000),
            Uint128::new(5000),
        ),
    ];

    let (mut app, cw_rate_limit_contract) = proper_instantiate(vec![PathMsg {
        contract_addr: Addr::unchecked(BRIDGE_CONTRACT),
        channel_id: format!("channel"),
        denom: format!("denom"),
        quotas,
    }]);

    // Sending to use the daily allowance
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 1000_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap();

    // Another packet is rate limited
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 1000_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap_err();

    // ... One day passes
    app.update_block(|b| {
        b.height += 10;
        b.time = b.time.plus_seconds(RESET_TIME_DAILY + 1)
    });

    // Sending the packet should work now
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 1000_u32.into()
    );

    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap();

    // Do that for 4 more days
    for _ in 1..4 {
        // ... One day passes
        app.update_block(|b| {
            b.height += 10;
            b.time = b.time.plus_seconds(RESET_TIME_DAILY + 1)
        });

        // Sending the packet should work now
        let msg = test_msg_send!(
            channel_id: format!("channel"),
            denom: format!("denom"),
            funds: 1000_u32.into()
        );
        let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
        app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
            .unwrap();
    }

    // ... One day passes
    app.update_block(|b| {
        b.height += 10;
        b.time = b.time.plus_seconds(RESET_TIME_DAILY + 1)
    });

    // We now have exceeded the weekly limit!  Even if the daily limit allows us, the weekly doesn't
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 1000_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap_err();

    // ... One week passes
    app.update_block(|b| {
        b.height += 10;
        b.time = b.time.plus_seconds(RESET_TIME_WEEKLY + 1)
    });

    // We can still can't send because the weekly and monthly limits are the same
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 1000_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap_err();

    // Waiting a week again, doesn't help!!
    // ... One week passes
    app.update_block(|b| {
        b.height += 10;
        b.time = b.time.plus_seconds(RESET_TIME_WEEKLY + 1)
    });

    // We can still can't send because the  monthly limit hasn't passed
    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 1000_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap_err();

    // Only after two more weeks we can send again
    app.update_block(|b| {
        b.height += 10;
        b.time = b.time.plus_seconds((RESET_TIME_WEEKLY * 2) + 1) // Two weeks
    });

    let msg = test_msg_send!(
        channel_id: format!("channel"),
        denom: format!("denom"),
        funds: 1000_u32.into()
    );
    let cosmos_msg = cw_rate_limit_contract.call(msg).unwrap();
    app.execute(Addr::unchecked(BRIDGE_CONTRACT), cosmos_msg)
        .unwrap();
}
