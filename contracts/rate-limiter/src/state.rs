use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint128};

use cw_storage_plus::Map;

use crate::{msg::QuotaMsg, ContractError};

#[cw_serde]
pub struct Path {
    pub contract: Addr,
    pub denom: String,
    pub channel: String,
}

impl Path {
    pub fn new(contract: &Addr, channel: impl Into<String>, denom: impl Into<String>) -> Self {
        Path {
            contract: contract.to_owned(),
            channel: channel.into(),
            denom: denom.into(),
        }
    }
}

impl From<Path> for (Addr, String, String) {
    fn from(path: Path) -> (Addr, String, String) {
        (path.contract, path.channel, path.denom)
    }
}

impl From<&Path> for (Addr, String, String) {
    fn from(path: &Path) -> (Addr, String, String) {
        (
            path.contract.to_owned(),
            path.channel.to_owned(),
            path.denom.to_owned(),
        )
    }
}

#[derive(Debug, Clone)]
pub enum FlowType {
    In,
    Out,
}

/// A Flow represents the transfer of value for a denom through an contract bridge
/// during a time window.
///
/// It tracks inflows (transfers into osmosis) and outflows (transfers out of
/// osmosis).
///
/// The period_end represents the last point in time for which this Flow is
/// tracking the value transfer.
///
/// Periods are discrete repeating windows. A period only starts when a contract
/// call to update the Flow (SendPacket/RecvPackt) is made, and not right after
/// the period ends. This means that if no calls happen after a period expires,
/// the next period will begin at the time of the next call and be valid for the
/// specified duration for the quota.
///
/// This is a design decision to avoid the period calculations and thus reduce gas consumption
#[cw_serde]
pub struct Flow {
    pub inflow: Uint128,
    pub outflow: Uint128,
    pub period_end: Timestamp,
}

impl Flow {
    pub fn new(
        inflow: impl Into<Uint128>,
        outflow: impl Into<Uint128>,
        now: Timestamp,
        duration: u64,
    ) -> Self {
        Self {
            inflow: inflow.into(),
            outflow: outflow.into(),
            period_end: now.plus_seconds(duration),
        }
    }

    /// The balance of a flow is how much absolute value for the denom has moved
    /// through the channel before period_end. It returns a tuple of
    /// (balance_in, balance_out) where balance_in in is how much has been
    /// transferred into the flow, and balance_out is how much value transferred
    /// out.
    pub fn balance(&self) -> (Uint128, Uint128) {
        (
            self.inflow.saturating_sub(self.outflow),
            self.outflow.saturating_sub(self.inflow),
        )
    }

    /// checks if the flow, in the current state, has exceeded a max allowance
    pub fn exceeds(&self, direction: &FlowType, max_inflow: Uint128, max_outflow: Uint128) -> bool {
        let (balance_in, balance_out) = self.balance();
        match direction {
            FlowType::In => balance_in > max_inflow,
            FlowType::Out => balance_out > max_outflow,
        }
    }

    /// returns the balance in a direction. This is used for displaying cleaner errors
    pub fn balance_on(&self, direction: &FlowType) -> Uint128 {
        let (balance_in, balance_out) = self.balance();
        match direction {
            FlowType::In => balance_in,
            FlowType::Out => balance_out,
        }
    }

    /// If now is greater than the period_end, the Flow is considered expired.
    pub fn is_expired(&self, now: Timestamp) -> bool {
        self.period_end < now
    }

    // Mutating methods

    /// Expire resets the Flow to start tracking the value transfer from the
    /// moment this method is called.
    pub fn expire(&mut self, now: Timestamp, duration: u64) {
        self.inflow = Uint128::from(0_u32);
        self.outflow = Uint128::from(0_u32);
        self.period_end = now.plus_seconds(duration);
    }

    /// Updates the current flow incrementing it by a transfer of value.
    pub fn add_flow(&mut self, direction: FlowType, value: Uint128) {
        match direction {
            FlowType::In => self.inflow = self.inflow.saturating_add(value),
            FlowType::Out => self.outflow = self.outflow.saturating_add(value),
        }
    }

    /// Updates the current flow reducing it by a transfer of value.
    pub fn undo_flow(&mut self, direction: FlowType, value: Uint128) {
        match direction {
            FlowType::In => self.inflow = self.inflow.saturating_sub(value),
            FlowType::Out => self.outflow = self.outflow.saturating_sub(value),
        }
    }

    /// Applies a transfer. If the Flow is expired (now > period_end), it will
    /// reset it before applying the transfer.
    fn apply_transfer(
        &mut self,
        direction: &FlowType,
        funds: Uint128,
        now: Timestamp,
        quota: &Quota,
    ) -> bool {
        let mut expired = false;
        if self.is_expired(now) {
            self.expire(now, quota.duration);
            expired = true;
        }
        self.add_flow(direction.clone(), funds);
        expired
    }
}

/// A Quota is the percentage of the denom's total value that can be transferred
/// through the channel in a given period of time (duration)
///
/// Percentages can be different for send and recv
///
/// The name of the quota is expected to be a human-readable representation of
/// the duration (i.e.: "weekly", "daily", "every-six-months", ...)
#[cw_serde]
pub struct Quota {
    pub name: String,
    pub max_send: Uint128,
    pub max_recv: Uint128,
    pub duration: u64,
}

impl Quota {
    /// Calculates the max capacity (absolute value in the same unit as
    /// total_value) in each direction based on the total value of the denom in
    /// the channel. The result tuple represents the max capacity when the
    /// transfer is in directions: (FlowType::In, FlowType::Out)
    pub fn capacity(&self) -> (Uint128, Uint128) {
        (self.max_recv, self.max_send)
    }

    /// returns the capacity in a direction. This is used for displaying cleaner errors
    pub fn capacity_on(&self, direction: &FlowType) -> Uint128 {
        let (max_in, max_out) = self.capacity();
        match direction {
            FlowType::In => max_in,
            FlowType::Out => max_out,
        }
    }
}

impl From<&QuotaMsg> for Quota {
    fn from(msg: &QuotaMsg) -> Self {
        Quota {
            name: msg.name.clone(),
            max_recv: msg.max_receive,
            max_send: msg.max_send,
            duration: msg.duration,
        }
    }
}

/// RateLimit is the main structure tracked for each channel/denom pair. Its quota
/// represents rate limit configuration, and the flow its
/// current state (i.e.: how much value has been transfered in the current period)
#[cw_serde]
pub struct RateLimit {
    pub quota: Quota,
    pub flow: Flow,
}

impl RateLimit {
    /// Checks if a transfer is allowed and updates the data structures
    /// accordingly.
    ///
    /// If the transfer is not allowed, it will return a RateLimitExceeded error.
    ///
    /// Otherwise it will return a RateLimitResponse with the updated data structures
    pub fn allow_transfer(
        &mut self,
        path: &Path,
        direction: &FlowType,
        funds: Uint128,
        now: Timestamp,
    ) -> Result<Self, ContractError> {
        // Flow used before this transaction is applied.
        // This is used to make error messages more informative
        let initial_flow = self.flow.balance_on(direction);

        // Apply the transfer. From here on, we will updated the flow with the new transfer
        // and check if  it exceeds the quota at the current time

        let _expired = self.flow.apply_transfer(direction, funds, now, &self.quota);

        let (max_in, max_out) = self.quota.capacity();
        // Return the effects of applying the transfer or an error.
        match self.flow.exceeds(direction, max_in, max_out) {
            true => Err(ContractError::RateLimitExceded {
                contract: path.contract.to_string(),
                channel: path.channel.to_string(),
                denom: path.denom.to_string(),
                amount: funds,
                quota_name: self.quota.name.to_string(),
                used: initial_flow,
                max: self.quota.capacity_on(direction),
                reset: self.flow.period_end,
            }),
            false => Ok(RateLimit {
                quota: self.quota.clone(), // Cloning here because self.quota.name (String) does not allow us to implement Copy
                flow: self.flow.clone(), // We can Copy flow, so this is slightly more efficient than cloning the whole RateLimit
            }),
        }
    }
}

/// RATE_LIMIT_TRACKERS is the main state for this contract. It maps a path (
/// Contract+ Channel + denom) to a vector of `RateLimit`s.
///
/// The `RateLimit` struct contains the information about how much value of a
/// denom has moved through the channel during the currently active time period
/// (channel_flow.flow) and what percentage of the denom's value we are
/// allowing to flow through that channel in a specific duration (quota)
///
/// For simplicity, the channel in the map keys refers to the "host" channel on
/// the osmosis side. This means that on PacketSend it will refer to the source
/// channel while on PacketRecv it refers to the destination channel.
///
/// It is the responsibility of the go module to pass the appropriate channel
/// when sending the messages
///
/// The map key (String, String) represents (channel_id, denom). We use
/// composite keys instead of a struct to avoid having to implement the
/// PrimaryKey trait
pub const RATE_LIMIT_TRACKERS: Map<(Addr, String, String), Vec<RateLimit>> = Map::new("flow");

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const RESET_TIME_DAILY: u64 = 60 * 60 * 24;
    pub const RESET_TIME_WEEKLY: u64 = 60 * 60 * 24 * 7;
    pub const RESET_TIME_MONTHLY: u64 = 60 * 60 * 24 * 30;

    #[test]
    fn flow() {
        let epoch = Timestamp::from_seconds(0);
        let mut flow = Flow::new(0_u32, 0_u32, epoch, RESET_TIME_WEEKLY);

        assert!(!flow.is_expired(epoch));
        assert!(!flow.is_expired(epoch.plus_seconds(RESET_TIME_DAILY)));
        assert!(!flow.is_expired(epoch.plus_seconds(RESET_TIME_WEEKLY)));
        assert!(flow.is_expired(epoch.plus_seconds(RESET_TIME_WEEKLY).plus_nanos(1)));

        assert_eq!(flow.balance(), (0_u32.into(), 0_u32.into()));
        flow.add_flow(FlowType::In, 5_u32.into());
        assert_eq!(flow.balance(), (5_u32.into(), 0_u32.into()));
        flow.add_flow(FlowType::Out, 2_u32.into());
        assert_eq!(flow.balance(), (3_u32.into(), 0_u32.into()));
        // Adding flow doesn't affect expiration
        assert!(!flow.is_expired(epoch.plus_seconds(RESET_TIME_DAILY)));

        flow.expire(epoch.plus_seconds(RESET_TIME_WEEKLY), RESET_TIME_WEEKLY);
        assert_eq!(flow.balance(), (0_u32.into(), 0_u32.into()));
        assert_eq!(flow.inflow, Uint128::from(0_u32));
        assert_eq!(flow.outflow, Uint128::from(0_u32));
        assert_eq!(flow.period_end, epoch.plus_seconds(RESET_TIME_WEEKLY * 2));

        // Expiration has moved
        assert!(!flow.is_expired(epoch.plus_seconds(RESET_TIME_WEEKLY).plus_nanos(1)));
        assert!(!flow.is_expired(epoch.plus_seconds(RESET_TIME_WEEKLY * 2)));
        assert!(flow.is_expired(epoch.plus_seconds(RESET_TIME_WEEKLY * 2).plus_nanos(1)));
    }
}
