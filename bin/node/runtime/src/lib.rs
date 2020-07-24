// Copyright 2018-2020 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate. If not, see <http://www.gnu.org/licenses/>.

//! The Substrate runtime. This can be compiled with ``#[no_std]`, ready for Wasm.

#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit="256"]

use sp_std::prelude::*;
use frame_support::{
	construct_runtime, parameter_types, debug,
	weights::{
		Weight,
		constants::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight, WEIGHT_PER_SECOND},
	},
	traits::{Currency, Imbalance, KeyOwnerProofSystem, OnUnbalanced, Randomness, LockIdentifier},
};
use sp_core::{
	crypto::KeyTypeId,
	u32_trait::{_1, _2, _3, _4},
	OpaqueMetadata,
};
pub use node_primitives::{AccountId, Signature};
use node_primitives::{AccountIndex, Balance, BlockNumber, Hash, Index, Moment, USD, Count, Duration,};
use sp_api::impl_runtime_apis;
use sp_runtime::{
	Permill, Perbill, Perquintill, Percent, ApplyExtrinsicResult,
	impl_opaque_keys, generic, create_runtime_str, ModuleId,

};
use sp_runtime::curve::PiecewiseLinear;
use sp_runtime::transaction_validity::{TransactionValidity, TransactionSource, TransactionPriority};
use sp_runtime::traits::{
	self, BlakeTwo256, Block as BlockT, StaticLookup, SaturatedConversion,
	ConvertInto, OpaqueKeys, NumberFor,
};
use sp_version::RuntimeVersion;
#[cfg(any(feature = "std", test))]
use sp_version::NativeVersion;
use pallet_grandpa::{AuthorityId as GrandpaId, AuthorityList as GrandpaAuthorityList};
use pallet_grandpa::fg_primitives;
use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
use sp_authority_discovery::AuthorityId as AuthorityDiscoveryId;
use pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo;
use pallet_contracts_rpc_runtime_api::ContractExecResult;
use pallet_session::{historical as pallet_session_historical};
use sp_inherents::{InherentData, CheckInherentsResult};

#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;
pub use pallet_timestamp::Call as TimestampCall;
pub use pallet_balances::Call as BalancesCall;
pub use frame_system::Call as SystemCall;
pub use pallet_contracts::Gas;
pub use frame_support::StorageValue;
pub use pallet_staking::StakerStatus;
use codec::Encode;

/// Implementations of some helper traits passed into runtime modules as associated types.
pub mod impls;
pub mod register;
pub mod report;
pub mod mine;
pub mod mine_linked;
pub mod mine_power;


pub mod offchain_common;
pub mod tx_valid;
pub mod address_valid;

pub use pallet_nicks;
use impls::{CurrencyToVoteHandler, Author, LinearWeightToFee, TargetedFeeAdjustment};

/// Constant values used within the runtime.
pub mod constants;
use constants::{time::*, currency::*, genesis_params::*};

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

/// Runtime version.
pub const VERSION: RuntimeVersion = RuntimeVersion {
	spec_name: create_runtime_str!("node"),
	impl_name: create_runtime_str!("transx-node"),
	authoring_version: 10,
	// Per convention: if the runtime behavior changes, increment spec_version
	// and set impl_version to 0. If only runtime
	// implementation changes and behavior does not, then leave spec_version as
	// is and increment impl_version.
	spec_version: 248,
	impl_version: 0,
	apis: RUNTIME_API_VERSIONS,
	transaction_version: 1
};

/// Native version.
#[cfg(any(feature = "std", test))]
pub fn native_version() -> NativeVersion {
	NativeVersion {
		runtime_version: VERSION,
		can_author_with: Default::default(),
	}
}

type NegativeImbalance = <Balances as Currency<AccountId>>::NegativeImbalance;

pub struct DealWithFees;
impl OnUnbalanced<NegativeImbalance> for DealWithFees {
	fn on_unbalanceds<B>(mut fees_then_tips: impl Iterator<Item=NegativeImbalance>) {
		if let Some(fees) = fees_then_tips.next() {
			// for fees, 80% to treasury, 20% to author
			let mut split = fees.ration(80, 20);
			if let Some(tips) = fees_then_tips.next() {
				// for tips, if any, 80% to treasury, 20% to author (though this can be anything)
				tips.ration_merge_into(80, 20, &mut split);
			}
			Treasury::on_unbalanced(split.0);
			Author::on_unbalanced(split.1);
		}
	}
}

parameter_types!{
	pub const ReservationFee: Balance = 10*DOLLARS;
	pub const MinLength: usize = 3;
	pub const MaxLength: usize = 20;

}

impl pallet_nicks::Trait for Runtime{
	type Event = Event;
	type Currency_n = Balances;
	type ReservationFee = ReservationFee;
	type Slashed = ();
	// 技术委员会
	type ForceOrigin = pallet_collective::EnsureMember<AccountId, TechnicalCollective>;
	type MinLength = MinLength;
	type MaxLength = MaxLength;

}

parameter_types!{
	pub const ProposalExpire: BlockNumber = 7*DAYS;
	pub const VoteRewardPeriod: BlockNumber = 1 * DAYS;
	pub const ReportReserve: Balance = 10*DOLLARS;
	pub const ReportReward: Balance = 250*DOLLARS;
	pub const IllegalPunishment: Balance = 500*DOLLARS;
	pub const CouncilReward: Balance = 10*DOLLARS;
	pub const Threshould: u32 = 7;
	pub const CancelReportSlash: Balance = 1*DOLLARS;
}

impl report::Trait for Runtime {
	type ConcilMembers = Council;
	type ConcilCount = Council;
	type ShouldAddOrigin = ();
	type ShouldSubOrigin = ();
	type CancelReportSlash = CancelReportSlash;
// 	type Thredshould = Threshould;
	type ConcilOrigin = pallet_collective::EnsureMember<AccountId, CouncilCollective>;
// 	type Currency0 = Balances;
	type ProposalExpire = ProposalExpire;
	type VoteRewardPeriod = VoteRewardPeriod;
	type ReportReserve = ReportReserve;
	type IllegalPunishment = IllegalPunishment;
	type CouncilReward = CouncilReward;
	type ReportReward = ReportReward;
	type Event = Event;
	type DeadOrigin = Balances;

}

parameter_types! {
	pub const ChangeAddressMaxCount: u32 = 2;
	pub const TxsMaxCount: u32 = 5;   // todo 上线调为 1000
	// 注册抵押金额
	pub const PledgeAmount: Balance = 20000 * DOLLARS;

	pub const UnBondingDuration: BlockNumber = 5 * MINUTES;
}

impl register::Trait for Runtime {

	type PledgeAmount = PledgeAmount;
	type Event = Event;
	type Currency1 = Balances;

	// 一条链下的地址最多更改次数（能够使用的情况下
	type ChangeAddressMaxCount = ChangeAddressMaxCount;
	type TxsMaxCount = TxsMaxCount;
	type UnBondingDuration = UnBondingDuration;
	}

parameter_types! {
	pub const TxFetchNumber:BlockNumber = 10; // todo:线上改为1小时清1次
//	pub const TwoHour:BlockNumber = 20 ; // TODO:上线环境改为 2 小时 2*HOURS
	pub const Hour:BlockNumber = HOURS;
}

//type SubmitTxValidTransaction = frame_system::offchain::TransactionSubmitter<
//	tx_valid::tx_crypto::AuthorityId,
//	Runtime,
//	UncheckedExtrinsic
//>;

impl offchain_common::BaseLocalAuthorityTrait for Runtime {
	type AuthorityId = address_valid::address_crypto::AuthorityId;
}

impl offchain_common::AdddressValidLocalAuthorityTrait for Runtime {
	type AuthorityId = address_valid::address_crypto::AuthorityId;
}

impl offchain_common::TxValidLocalAuthorityTrait for Runtime {
	type AuthorityId = tx_valid::tx_crypto::AuthorityId;
}

impl tx_valid::Trait for Runtime {
	type Event = Event;
//	type Call = Call;
//	type SubmitSignedTransaction = SubmitTxValidTransaction;
//	type SubmitUnsignedTransaction = SubmitTxValidTransaction;
//	type AuthorityId = tx_valid::tx_crypto::AuthorityId;
	type Duration = TxFetchNumber;
}

//type SubmitAddressValidTransaction = frame_system::offchain::TransactionSubmitter<
//	address_valid::address_crypto::AuthorityId,
//	Runtime,
//	UncheckedExtrinsic
//>;

impl address_valid::Trait for Runtime {
	type Event = Event;
//	type Call = Call;
//	type SubmitSignedTransaction = SubmitAddressValidTransaction;
//	type SubmitUnsignedTransaction = SubmitAddressValidTransaction;
//	type AuthorityId = address_valid::address_crypto::AuthorityId;
	type Duration = TxFetchNumber;
	type UnsignedPriority = OffchainWorkUnsignedPriority;
}

parameter_types! {
	pub const TranRuntime: Runtime = Runtime;
	// 将算力汇总信息归档到链上并不再修改
	pub const ArchiveDuration: BlockNumber = ArchiveDurationTime;
	pub const RemovePersonRecordDuration: BlockNumber = 7*DAYS;

	// 挖矿奖励金额
	pub const FirstYearPerDayMineRewardToken: Balance = 2100_0000*DOLLARS/2/4/36525*100; // 这里一年直接用365.25天来算

	pub const Alpha: Permill = Permill::from_percent(50);
	pub const AmountPowerPortionRatio: Permill = Permill::from_percent(50);

	// 这个数额是多少需要斟酌
	//*********************************************************************************************
	pub const MoreThanPortionNeedMinCount: u64 = 100;
	pub const MoreThanPortionNeedMinAmount:u64 = USDT_DECIMALS * INIT_AMOUNT_POWER * 2 * 100; // 次数的10倍

	pub const SuperiorInflationRatio: Permill = Permill::from_percent(25);
	pub const FatherInflationRatio: Permill = Permill::from_percent(50);

	pub const FoundationShareRatio: u32 = 20;
	pub const MinerSharePortion: u32 = 100;
	pub const FatherSharePortion: u32 = 50;
	pub const SuperSharePortion: u32 = 25;
	// ********************************************************************************************
	pub const MLAbtc: USD = 10_0000 * USDT_DECIMALS;
	pub const MLAeth: USD = 4_0000 * USDT_DECIMALS;
	pub const MLAeos: USD = 1_0000 * USDT_DECIMALS;
	pub const MLAusdt: USD = 5000 * USDT_DECIMALS;
	pub const MLAecap: USD = 5000 * 2 * USDT_DECIMALS;

	// 个人算力硬顶
	pub const LAbtc: USD = 100_0000_00000;
	pub const LCbtc: Count = 100_0000;

	pub const LAeth: USD = 100_0000_00000;
	pub const LCeth: Count = 100_0000;

	pub const LAusdt: USD = 100_0000_00000;
	pub const LCusdt: Count = 100_0000;

	pub const LAeos: USD = 100_0000_00000;
	pub const LCeos: Count = 100_0000;

	pub const LAecap: USD = 100_0000_00000;
	pub const LCecap: Count = 100_0000;


	pub const ClientWorkPowerRatio: u64 = 50;
	pub const PerDayMinReward: Balance = 100*DOLLARS;

	// 全网挖矿次数硬顶
	pub const BTCLimitCount: Count = 10000;
	pub const ETHLimitCount: Count = 10000;
	pub const EOSLimitCount: Count = 10000;
	pub const USDTLimitCount: Count = 10000;
	pub const ECAPLimitCount: Count = 10000;

	// 全网token挖矿金额硬顶
	pub const BTCLimitAmount: USD = 10000 * INIT_AMOUNT_POWER * USDT_DECIMALS;
	pub const ETHLimitAmount: USD = 10000 * INIT_AMOUNT_POWER * USDT_DECIMALS;
	pub const EOSLimitAmount: USD = 10000 * INIT_AMOUNT_POWER * USDT_DECIMALS;
	pub const USDTLimitAmount: USD = 10000 * INIT_AMOUNT_POWER * USDT_DECIMALS;
	pub const ECAPLimitAmount: USD = 10000 * INIT_AMOUNT_POWER * USDT_DECIMALS;

	pub const SubHalfDuration:Duration = 4; // 四年减半

	// 全网token挖矿占比硬顶
	pub const BTCMaxPortion: Permill = Permill::from_percent(70);
	pub const ETHMaxPortion: Permill = Permill::from_percent(10);
	pub const EOSMaxPortion: Permill = Permill::from_percent(8);
	pub const USDTMaxPortion: Permill = Permill::from_percent(50);
	pub const ECAPMaxPortion: Permill = Permill::from_percent(50);

	pub const MiningMaxNum: Count = 100;  // 一个人最大挖矿次数可以是100笔

	pub const Multiple: u64 = 1_0000;

	pub const ZeroDayAmount: u64 = INIT_AMOUNT_POWER * USDT_DECIMALS * INIT_MINER_COUNT;  // 是最小转账单位 * 每个人平均初始算力 * 20个人

	pub const ZeroDayCount: u64 = 1 * INIT_COUNT_POWER * INIT_MINER_COUNT;  // 假设金额是次数的20倍

	// 这个值的范围是11～20
	pub const DeclineExp: u64 = 12;

}

impl mine::Trait for Runtime {

	type ReportedTxs = Report;
	type TechMmebersOrigin = TechnicalCommittee; // 获取所有技术委员会成员
	type ShouldAddOrigin = ();
	type Event = Event;
	type Currency3 = Balances;
	type MineIndex = u64;
	type ArchiveDuration = ArchiveDuration;
	type RemovePersonRecordDuration = RemovePersonRecordDuration;

	type FirstYearPerDayMineRewardToken = FirstYearPerDayMineRewardToken;

	type BTCLimitCount = BTCLimitCount;
	type BTCLimitAmount =  BTCLimitAmount;

	type ETHLimitCount = ETHLimitCount;
	type ETHLimitAmount =  ETHLimitAmount;

	type EOSLimitCount = EOSLimitCount;
	type EOSLimitAmount =  EOSLimitAmount;

	type USDTLimitCount = USDTLimitCount;
	type USDTLimitAmount =  USDTLimitAmount;

	type ECAPLimitCount = ECAPLimitCount;
	type ECAPLimitAmount =  ECAPLimitAmount;


	type MiningMaxNum = MiningMaxNum;

	type BTCMaxPortion = BTCMaxPortion;
	type ETHMaxPortion = ETHMaxPortion;
	type EOSMaxPortion = EOSMaxPortion;
	type USDTMaxPortion = USDTMaxPortion;
	type ECAPMaxPortion = ECAPMaxPortion;

	// 单次转账金额硬顶
	type MLAbtc = MLAbtc;
	type MLAusdt = MLAusdt;
	type MLAeos = MLAeos;
	type MLAeth = MLAeth;
	type MLAecap = MLAecap;

	// 个人算力硬顶
	type LAbtc = LAbtc;
	type LCbtc = LCbtc;

	type LAeth = LAeth;
	type LCeth = LCeth;

	type LAusdt = LAusdt;
	type LCusdt = LCusdt;

	type LAeos = LAeos;
	type LCeos = LCeos;

	type LAecap = LAecap;
	type LCecap = LCecap;



	type SuperiorInflationRatio = SuperiorInflationRatio;
	type FatherInflationRatio = FatherInflationRatio;

	type  SubHalfDuration = SubHalfDuration; // 减半周期 如果是四年直接写4 5年直接写5

	//***以下是可治理的参数***
	type Alpha = Alpha;  // 钝化系数(如果是0.3就写30， 0.5就写50，如此类推)
	type AmountPowerPortionRatio = AmountPowerPortionRatio;  // 金额算力的占比系数(如果是0.3就写30， 0.5就写50，如此类推)

	// 创始团队成员的分润占比（20% 写20； 25% 写25；以此类推）
	type FoundationShareRatio = FoundationShareRatio;
	// ***注意 下面的值不是占比 占比在相应方法中计算  如果矿工是100， 上级是50， 上上级是25， 那么
	// 矿工的分润比就是 100 / （100 + 50 + 25）
	// 矿工奖励部分
	type MinerSharePortion = MinerSharePortion; //
	// 上级的奖励部分
	type FatherSharePortion = FatherSharePortion;
	// 上上级的奖励部分
	type SuperSharePortion = SuperSharePortion;

	// 客户端挖矿占比
	type ClientWorkPowerRatio = ClientWorkPowerRatio;

	// 单日全网最低挖矿奖励
	type PerDayMinReward = PerDayMinReward;

	// 算力相对于金额与次数的倍数
	type Multiple = Multiple;

	// 第一天挖矿初始化金额
	type ZeroDayAmount = ZeroDayAmount;

	// 第一天挖矿初始化频次
	type ZeroDayCount = ZeroDayCount;

	// 钝化用到的下降指数
	type DeclineExp = DeclineExp;

}

parameter_types! {
	pub const BlockHashCount: BlockNumber = 250;
	/// We allow for 2 seconds of compute with a 6 second average block time.
	pub const MaximumBlockWeight: Weight = 2 * WEIGHT_PER_SECOND;
	pub const MaximumBlockLength: u32 = 5 * 1024 * 1024;
	pub const Version: RuntimeVersion = VERSION;
	pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
}

impl frame_system::Trait for Runtime {
	type Origin = Origin;
	type Call = Call;
	type Index = Index;
	type BlockNumber = BlockNumber;
	type Hash = Hash;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = Indices;
	type Header = generic::Header<BlockNumber, BlakeTwo256>;
	type Event = Event;
	type BlockHashCount = BlockHashCount;
	type MaximumBlockWeight = MaximumBlockWeight;
	type DbWeight = RocksDbWeight;
	type BlockExecutionWeight = BlockExecutionWeight;
	type ExtrinsicBaseWeight = ExtrinsicBaseWeight;
	type MaximumBlockLength = MaximumBlockLength;
	type AvailableBlockRatio = AvailableBlockRatio;
	type Version = Version;
	type ModuleToIndex = ModuleToIndex;
	type AccountData = pallet_balances::AccountData<Balance>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
}

parameter_types! {
	// One storage item; value is size 4+4+16+32 bytes = 56 bytes.
	pub const MultisigDepositBase: Balance = 30 * CENTS;
	// Additional storage item size of 32 bytes.
	pub const MultisigDepositFactor: Balance = 5 * CENTS;
	pub const MaxSignatories: u16 = 100;
}

impl pallet_utility::Trait for Runtime {
	type Event = Event;
	type Call = Call;
	type Currency = Balances;
	type MultisigDepositBase = MultisigDepositBase;
	type MultisigDepositFactor = MultisigDepositFactor;
	type MaxSignatories = MaxSignatories;
}

parameter_types! {
	pub const MaximumSchedulerWeight: Weight = Perbill::from_percent(80) * MaximumBlockWeight::get();
}

impl pallet_scheduler::Trait for Runtime {
	type Event = Event;
	type Origin = Origin;
	type Call = Call;
	type MaximumWeight = MaximumSchedulerWeight;
}

parameter_types! {
	pub const EpochDuration: u64 = EPOCH_DURATION_IN_SLOTS;
	pub const ExpectedBlockTime: Moment = MILLISECS_PER_BLOCK;
}

impl pallet_babe::Trait for Runtime {
	type EpochDuration = EpochDuration;
	type ExpectedBlockTime = ExpectedBlockTime;
	type EpochChangeTrigger = pallet_babe::ExternalTrigger;
}

parameter_types! {
	pub const IndexDeposit: Balance = 1 * DOLLARS;
}

impl pallet_indices::Trait for Runtime {
	type AccountIndex = AccountIndex;
	type Event = Event;
	type Currency = Balances;
	type Deposit = IndexDeposit;
}

parameter_types! {
	pub const ExistentialDeposit: Balance = 1 * DOLLARS;
}

impl pallet_balances::Trait for Runtime {
	type Balance = Balance;
	type DustRemoval = ();
	type Event = Event;
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = frame_system::Module<Runtime>;
}

parameter_types! {
	pub const TransactionByteFee: Balance = 10 * MILLICENTS;
	// In the Substrate node, a weight of 10_000_000 (smallest non-zero weight)
	// is mapped to 10_000_000 units of fees, hence:
	pub const WeightFeeCoefficient: Balance = 1;
	// for a sane configuration, this should always be less than `AvailableBlockRatio`.
	pub const TargetBlockFullness: Perquintill = Perquintill::from_percent(25);
}

impl pallet_transaction_payment::Trait for Runtime {
	type Currency = Balances;
	type OnTransactionPayment = DealWithFees;
	type TransactionByteFee = TransactionByteFee;
	type WeightToFee = LinearWeightToFee<WeightFeeCoefficient>;
	type FeeMultiplierUpdate = TargetedFeeAdjustment<TargetBlockFullness>;
}

parameter_types! {
	pub const MinimumPeriod: Moment = SLOT_DURATION / 2;
}

impl pallet_timestamp::Trait for Runtime {
	type Moment = Moment;
	type OnTimestampSet = Babe;
	type MinimumPeriod = MinimumPeriod;
}

parameter_types! {
	pub const UncleGenerations: BlockNumber = 5;
}

impl pallet_authorship::Trait for Runtime {
	type FindAuthor = pallet_session::FindAccountFromAuthorIndex<Self, Babe>;
	type UncleGenerations = UncleGenerations;
	type FilterUncle = ();
	type EventHandler = (Staking, ImOnline);
}

impl_opaque_keys! {
	pub struct SessionKeys {
		pub grandpa: Grandpa,
		pub babe: Babe,
		pub im_online: ImOnline,
		pub authority_discovery: AuthorityDiscovery,
	}
}

parameter_types! {
	pub const DisabledValidatorsThreshold: Perbill = Perbill::from_percent(17);
}

impl pallet_session::Trait for Runtime {
	type Event = Event;
	type ValidatorId = <Self as frame_system::Trait>::AccountId;
	type ValidatorIdOf = pallet_staking::StashOf<Self>;
	type ShouldEndSession = Babe;
	type SessionManager = pallet_session::historical::NoteHistoricalRoot<Self, Staking>;
	type SessionHandler = <SessionKeys as OpaqueKeys>::KeyTypeIdProviders;
	type Keys = SessionKeys;
	type DisabledValidatorsThreshold = DisabledValidatorsThreshold;
	type NextSessionRotation = Babe;
}

impl pallet_session::historical::Trait for Runtime {
	type FullIdentification = pallet_staking::Exposure<AccountId, Balance>;
	type FullIdentificationOf = pallet_staking::ExposureOf<Runtime>;
}

pallet_staking_reward_curve::build! {
	const REWARD_CURVE: PiecewiseLinear<'static> = curve!(
		min_inflation: 0_025_000,
		max_inflation: 0_100_000,
		ideal_stake: 0_500_000,
		falloff: 0_050_000,
		max_piece_count: 40,
		test_precision: 0_005_000,
	);
}

parameter_types! {
	pub const SessionsPerEra: sp_staking::SessionIndex = 6;
	pub const BondingDuration: pallet_staking::EraIndex = 24 * 28;
	pub const SlashDeferDuration: pallet_staking::EraIndex = 24 * 7; // 1/4 the bonding duration.
	pub const RewardCurve: &'static PiecewiseLinear<'static> = &REWARD_CURVE;
	pub const ElectionLookahead: BlockNumber = EPOCH_DURATION_IN_BLOCKS / 4;
	pub const MaxNominatorRewardedPerValidator: u32 = 64;
	pub const MaxIterations: u32 = 5;
}

impl pallet_staking::Trait for Runtime {
	type Currency = Balances;
	type UnixTime = Timestamp;
	type CurrencyToVote = CurrencyToVoteHandler;
	type RewardRemainder = Treasury;
	type Event = Event;
	type Slash = Treasury; // send the slashed funds to the treasury.
	type Reward = (); // rewards are minted from the void
	type SessionsPerEra = SessionsPerEra;
	type BondingDuration = BondingDuration;
	type SlashDeferDuration = SlashDeferDuration;
	/// A super-majority of the council can cancel the slash.
	type SlashCancelOrigin = pallet_collective::EnsureProportionAtLeast<_3, _4, AccountId, CouncilCollective>;
	type SessionInterface = Self;
	type RewardCurve = RewardCurve;
	type NextNewSession = Session;
	type ElectionLookahead = ElectionLookahead;
	type Call = Call;
	type MaxIterations = MaxIterations;
	type MaxNominatorRewardedPerValidator = MaxNominatorRewardedPerValidator;
	type UnsignedPriority = StakingUnsignedPriority;
}

parameter_types! {
	pub const LaunchPeriod: BlockNumber = 10 * MINUTES;
	pub const VotingPeriod: BlockNumber = 5 * MINUTES;
	pub const FastTrackVotingPeriod: BlockNumber = 5 * MINUTES;
	pub const InstantAllowed: bool = true;
	pub const MinimumDeposit: Balance = 100 * DOLLARS;
	pub const EnactmentPeriod: BlockNumber = 5 * MINUTES;
	pub const CooloffPeriod: BlockNumber = 5 * MINUTES;
	// One cent: $10,000 / MB
	pub const PreimageByteDeposit: Balance = 1 * CENTS;
}

impl pallet_democracy::Trait for Runtime {
	type Proposal = Call;
	type Event = Event;
	type Currency = Balances;
	type EnactmentPeriod = EnactmentPeriod;
	type LaunchPeriod = LaunchPeriod;
	type VotingPeriod = VotingPeriod;
	type MinimumDeposit = MinimumDeposit;
	/// A straight majority of the council can decide what their next motion is.
	type ExternalOrigin = pallet_collective::EnsureProportionAtLeast<_1, _2, AccountId, CouncilCollective>;
	/// A super-majority can have the next scheduled referendum be a straight majority-carries vote.
	type ExternalMajorityOrigin = pallet_collective::EnsureProportionAtLeast<_3, _4, AccountId, CouncilCollective>;
	/// A unanimous council can have the next scheduled referendum be a straight default-carries
	/// (NTB) vote.
	type ExternalDefaultOrigin = pallet_collective::EnsureProportionAtLeast<_1, _1, AccountId, CouncilCollective>;
	/// Two thirds of the technical committee can have an ExternalMajority/ExternalDefault vote
	/// be tabled immediately and with a shorter voting/enactment period.
	type FastTrackOrigin = pallet_collective::EnsureProportionAtLeast<_2, _3, AccountId, TechnicalCollective>;
	type InstantOrigin = pallet_collective::EnsureProportionAtLeast<_1, _1, AccountId, TechnicalCollective>;
	type InstantAllowed = InstantAllowed;
	type FastTrackVotingPeriod = FastTrackVotingPeriod;
	// To cancel a proposal which has been passed, 2/3 of the council must agree to it.
	type CancellationOrigin = pallet_collective::EnsureProportionAtLeast<_2, _3, AccountId, CouncilCollective>;
	// Any single technical committee member may veto a coming council proposal, however they can
	// only do it once and it lasts only for the cooloff period.
	type VetoOrigin = pallet_collective::EnsureMember<AccountId, TechnicalCollective>;
	type CooloffPeriod = CooloffPeriod;
	type PreimageByteDeposit = PreimageByteDeposit;
	type Slash = Treasury;
	type Scheduler = Scheduler;
}

parameter_types! {
	pub const CouncilMotionDuration: BlockNumber = 5 * DAYS;
}

type CouncilCollective = pallet_collective::Instance1;
impl pallet_collective::Trait<CouncilCollective> for Runtime {
	type Origin = Origin;
	type Proposal = Call;
	type Event = Event;
	type MotionDuration = CouncilMotionDuration;
}

parameter_types! {
	pub const CandidacyBond: Balance = 10 * DOLLARS;
	pub const VotingBond: Balance = 1 * DOLLARS;
	pub const TermDuration: BlockNumber = 20 * MINUTES;//7 * DAYS;
	pub const DesiredMembers: u32 = 7;
	pub const DesiredRunnersUp: u32 = 4;
	pub const ElectionsPhragmenModuleId: LockIdentifier = *b"phrelect";
}

impl pallet_elections_phragmen::Trait for Runtime {
	type ModuleId = ElectionsPhragmenModuleId;
	type Event = Event;
	type Currency = Balances;
	type ChangeMembers = Council;
	// NOTE: this implies that council's genesis members cannot be set directly and must come from
	// this module.
	type InitializeMembers = Council;
	type CurrencyToVote = CurrencyToVoteHandler;
	type CandidacyBond = CandidacyBond;
	type VotingBond = VotingBond;
	type LoserCandidate = ();
	type BadReport = ();
	type KickedMember = ();
	type DesiredMembers = DesiredMembers;
	type DesiredRunnersUp = DesiredRunnersUp;
	type TermDuration = TermDuration;
}

parameter_types! {
	pub const TransxFoundationMotionDuration: BlockNumber = 5 * DAYS;
}
// transx基金会
type TransxFoundation = pallet_collective::Instance3;
impl pallet_collective::Trait<TransxFoundation> for Runtime {  // 这个写法很特殊

	type Origin = Origin;
	type Proposal = Call;
	type Event = Event;
	// 现在的议会多了一个时间参数
	type MotionDuration = TransxFoundationMotionDuration;
}

parameter_types! {
	pub const TechnicalMotionDuration: BlockNumber = 5 * DAYS;
}


type TechnicalCollective = pallet_collective::Instance2;
impl pallet_collective::Trait<TechnicalCollective> for Runtime {
	type Origin = Origin;
	type Proposal = Call;
	type Event = Event;
	type MotionDuration = TechnicalMotionDuration;
}

impl pallet_membership::Trait<pallet_membership::Instance1> for Runtime {
	type Event = Event;
	type AddOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
	type RemoveOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
	type SwapOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
	type ResetOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
	type PrimeOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
	type MembershipInitialized = TechnicalCommittee;
	type MembershipChanged = TechnicalCommittee;
}

parameter_types! {
	pub const ProposalBond: Permill = Permill::from_percent(5);
	pub const ProposalBondMinimum: Balance = 1 * DOLLARS;
	pub const SpendPeriod: BlockNumber = 1 * DAYS;
	pub const Burn: Permill = Permill::from_percent(0);  // 不需要销毁
	pub const TipCountdown: BlockNumber = 1 * DAYS;
	pub const TipFindersFee: Percent = Percent::from_percent(20);
	pub const TipReportDepositBase: Balance = 1 * DOLLARS;
	pub const TipReportDepositPerByte: Balance = 1 * CENTS;
	pub const TreasuryModuleId: ModuleId = ModuleId(*b"py/trsry");
}

impl pallet_treasury::Trait for Runtime {
	type Currency = Balances;
	type ApproveOrigin = pallet_collective::EnsureMembers<_4, AccountId, CouncilCollective>;
	type RejectOrigin = pallet_collective::EnsureMembers<_2, AccountId, CouncilCollective>;
	type Tippers = Elections;
	type TipCountdown = TipCountdown;
	type TipFindersFee = TipFindersFee;
	type TipReportDepositBase = TipReportDepositBase;
	type TipReportDepositPerByte = TipReportDepositPerByte;
	type Event = Event;
	type ShouldAddOrigin =();
	type ProposalRejection = ();
	type ProposalBond = ProposalBond;
	type ProposalBondMinimum = ProposalBondMinimum;
	type SpendPeriod = SpendPeriod;
	type Burn = Burn;
	type ModuleId = TreasuryModuleId;
}

parameter_types! {
	pub const TombstoneDeposit: Balance = 1 * DOLLARS;
	pub const RentByteFee: Balance = 1 * DOLLARS;
	pub const RentDepositOffset: Balance = 1000 * DOLLARS;
	pub const SurchargeReward: Balance = 150 * DOLLARS;
}

impl pallet_contracts::Trait for Runtime {
	type Time = Timestamp;
	type Randomness = RandomnessCollectiveFlip;
	type Call = Call;
	type Event = Event;
	type DetermineContractAddress = pallet_contracts::SimpleAddressDeterminer<Runtime>;
	type TrieIdGenerator = pallet_contracts::TrieIdFromParentCounter<Runtime>;
	type RentPayment = ();
	type SignedClaimHandicap = pallet_contracts::DefaultSignedClaimHandicap;
	type TombstoneDeposit = TombstoneDeposit;
	type StorageSizeOffset = pallet_contracts::DefaultStorageSizeOffset;
	type RentByteFee = RentByteFee;
	type RentDepositOffset = RentDepositOffset;
	type SurchargeReward = SurchargeReward;
	type MaxDepth = pallet_contracts::DefaultMaxDepth;
	type MaxValueSize = pallet_contracts::DefaultMaxValueSize;
}

impl pallet_sudo::Trait for Runtime {
	type Event = Event;
	type Call = Call;
}

parameter_types! {
	pub const SessionDuration: BlockNumber = EPOCH_DURATION_IN_SLOTS as _;
	pub const ImOnlineUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
	/// We prioritize im-online heartbeats over phragmen solution submission.
	pub const StakingUnsignedPriority: TransactionPriority = TransactionPriority::max_value() / 2;
	pub const OffchainWorkUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
}


impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Runtime where
	Call: From<LocalCall>,
{
	fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
		call: Call,
		public: <Signature as traits::Verify>::Signer,
		account: AccountId,
		nonce: Index,
	) -> Option<(Call, <UncheckedExtrinsic as traits::Extrinsic>::SignaturePayload)> {
		// take the biggest period possible.
		let period = BlockHashCount::get()
			.checked_next_power_of_two()
			.map(|c| c / 2)
			.unwrap_or(2) as u64;
		let current_block = System::block_number()
			.saturated_into::<u64>()
			// The `System::block_number` is initialized with `n+1`,
			// so the actual block number is `n`.
			.saturating_sub(1);
		let tip = 0;
		let extra: SignedExtra = (
			frame_system::CheckVersion::<Runtime>::new(),
			frame_system::CheckGenesis::<Runtime>::new(),
			frame_system::CheckEra::<Runtime>::from(generic::Era::mortal(period, current_block)),
			frame_system::CheckNonce::<Runtime>::from(nonce),
			frame_system::CheckWeight::<Runtime>::new(),
			pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(tip),
			pallet_grandpa::ValidateEquivocationReport::<Runtime>::new(),
		);
		let raw_payload = SignedPayload::new(call, extra).map_err(|e| {
			debug::warn!("Unable to create signed payload: {:?}", e);
		}).ok()?;
		let signature = raw_payload.using_encoded(|payload| {
			C::sign(payload, public)
		})?;
		let address = Indices::unlookup(account);
		let (call, extra, _) = raw_payload.deconstruct();
		Some((call, (address, signature.into(), extra)))
	}
}

impl frame_system::offchain::SigningTypes for Runtime {
	type Public = <Signature as traits::Verify>::Signer;
	type Signature = Signature;
}

impl<C> frame_system::offchain::SendTransactionTypes<C> for Runtime where
	Call: From<C>,
{
	type OverarchingCall = Call;
	type Extrinsic = UncheckedExtrinsic;
}

impl pallet_im_online::Trait for Runtime {
	type AuthorityId = ImOnlineId;
	type Event = Event;
	type SessionDuration = SessionDuration;
	type ReportUnresponsiveness = Offences;
	type UnsignedPriority = ImOnlineUnsignedPriority;
}

impl pallet_offences::Trait for Runtime {
	type Event = Event;
	type IdentificationTuple = pallet_session::historical::IdentificationTuple<Self>;
	type OnOffenceHandler = Staking;
}

impl pallet_authority_discovery::Trait for Runtime {}

impl pallet_grandpa::Trait for Runtime {
	type Event = Event;
	type Call = Call;

	type KeyOwnerProofSystem = Historical;

	type KeyOwnerProof =
		<Self::KeyOwnerProofSystem as KeyOwnerProofSystem<(KeyTypeId, GrandpaId)>>::Proof;

	type KeyOwnerIdentification = <Self::KeyOwnerProofSystem as KeyOwnerProofSystem<(
		KeyTypeId,
		GrandpaId,
	)>>::IdentificationTuple;

	type HandleEquivocation = pallet_grandpa::EquivocationHandler<
		Self::KeyOwnerIdentification,
		node_primitives::report::ReporterAppCrypto,
		Runtime,
		Offences,
	>;
}

parameter_types! {
	pub const WindowSize: BlockNumber = 101;
	pub const ReportLatency: BlockNumber = 1000;
}

impl pallet_finality_tracker::Trait for Runtime {
	type OnFinalizationStalled = ();
	type WindowSize = WindowSize;
	type ReportLatency = ReportLatency;
}

parameter_types! {
	pub const BasicDeposit: Balance = 10 * DOLLARS;       // 258 bytes on-chain
	pub const FieldDeposit: Balance = 250 * CENTS;        // 66 bytes on-chain
	pub const SubAccountDeposit: Balance = 2 * DOLLARS;   // 53 bytes on-chain
	pub const MaxSubAccounts: u32 = 100;
	pub const MaxAdditionalFields: u32 = 100;
	pub const MaxRegistrars: u32 = 20;
}

impl pallet_identity::Trait for Runtime {
	type Event = Event;
	type Currency = Balances;
	type BasicDeposit = BasicDeposit;
	type FieldDeposit = FieldDeposit;
	type SubAccountDeposit = SubAccountDeposit;
	type MaxSubAccounts = MaxSubAccounts;
	type MaxAdditionalFields = MaxAdditionalFields;
	type MaxRegistrars = MaxRegistrars;
	type Slashed = Treasury;
	type ForceOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
	type RegistrarOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
}

parameter_types! {
	pub const ConfigDepositBase: Balance = 5 * DOLLARS;
	pub const FriendDepositFactor: Balance = 50 * CENTS;
	pub const MaxFriends: u16 = 9;
	pub const RecoveryDeposit: Balance = 5 * DOLLARS;
}

impl pallet_recovery::Trait for Runtime {
	type Event = Event;
	type Call = Call;
	type Currency = Balances;
	type ConfigDepositBase = ConfigDepositBase;
	type FriendDepositFactor = FriendDepositFactor;
	type MaxFriends = MaxFriends;
	type RecoveryDeposit = RecoveryDeposit;
}

parameter_types! {
	pub const CandidateDeposit: Balance = 10 * DOLLARS;
	pub const WrongSideDeduction: Balance = 2 * DOLLARS;
	pub const MaxStrikes: u32 = 10;
	pub const RotationPeriod: BlockNumber = 80 * HOURS;
	pub const PeriodSpend: Balance = 500 * DOLLARS;
	pub const MaxLockDuration: BlockNumber = 36 * 30 * DAYS;
	pub const ChallengePeriod: BlockNumber = 7 * DAYS;
	pub const SocietyModuleId: ModuleId = ModuleId(*b"py/socie");
}

impl pallet_society::Trait for Runtime {
	type Event = Event;
	type Currency = Balances;
	type Randomness = RandomnessCollectiveFlip;
	type CandidateDeposit = CandidateDeposit;
	type WrongSideDeduction = WrongSideDeduction;
	type MaxStrikes = MaxStrikes;
	type PeriodSpend = PeriodSpend;
	type MembershipChanged = ();
	type RotationPeriod = RotationPeriod;
	type MaxLockDuration = MaxLockDuration;
	type FounderSetOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
	type SuspensionJudgementOrigin = pallet_society::EnsureFounder<Runtime>;
	type ChallengePeriod = ChallengePeriod;
	type ModuleId = SocietyModuleId;
}

parameter_types! {
	pub const MinVestedTransfer: Balance = 100 * DOLLARS;
}

impl pallet_vesting::Trait for Runtime {
	type Event = Event;
	type Currency = Balances;
	type BlockNumberToBalance = ConvertInto;
	type MinVestedTransfer = MinVestedTransfer;
}

parameter_types! {
	pub const MintExistsHowLong: BlockNumber = 10 * MINUTES;
	pub const MintPeriod: BlockNumber = 1 * HOURS;
	pub const BurnExistsHowLong: BlockNumber = 10 * MINUTES;
	pub const MintMinAmount: Balance = 10000 * DOLLARS;
	pub const BurnMinAmount: Balance = 1000 * DOLLARS;
	pub const MintPledge: Balance = 1 * DOLLARS;
	pub const BurnPledge: Balance = 1 * DOLLARS;
	pub const MaxLenOfMint: u32 = 3u32;
	pub const MaxLenOfBurn: u32  = 3u32;
}
impl generic_asset::Trait for Runtime{

//	type ShouldAddOrigin = ();
//	type ShouldSubOrigin = ();

	type CouncilMembers = Council;
	// 用来获取议会成员数目的
	type MembersCount = Council;
	// 铸币抵押的金额
	type MintPledge = MintPledge;
	// 销毁币抵押金额
	type BurnPledge = BurnPledge;
	// 铸币最小金额
	type MintMinAmount = MintMinAmount;
	// 销毁币最小金额
	type BurnMinAmount = BurnMinAmount;
	// 铸币议案存在的最长时间
	type MintExistsHowLong = MintExistsHowLong;
	// 铸币议案统一处理的时间
	type MintPeriod = MintPeriod;
	// 销毁币议案存在的最长时间
	type BurnExistsHowLong = BurnExistsHowLong;
	type Currency = Balances;
	type Event = Event;
	type Balance = u128;
	type AssetId = u32;
	// 用于判断是否是议会成员
	type CouncilOrigin = pallet_collective::EnsureMember<AccountId, CouncilCollective>;
	// 用于判断是否是技术委员会成员
	type TechnicalOrigin = pallet_collective::EnsureMember<AccountId, TechnicalCollective>;
	// transx基金会
	type TransxFoundation = pallet_collective::EnsureMember<AccountId, TransxFoundation>;

	type MaxLenOfMint  = MaxLenOfMint;
	type MaxLenOfBurn =  MaxLenOfBurn;
	type TreasuryId = TreasuryModuleId;

}


construct_runtime!(
	pub enum Runtime where
		Block = Block,
		NodeBlock = node_primitives::Block,
		UncheckedExtrinsic = UncheckedExtrinsic
	{
		System: frame_system::{Module, Call, Config, Storage, Event<T>},
		Utility: pallet_utility::{Module, Call, Storage, Event<T>},
		Babe: pallet_babe::{Module, Call, Storage, Config, Inherent(Timestamp)},
		Timestamp: pallet_timestamp::{Module, Call, Storage, Inherent},
		Authorship: pallet_authorship::{Module, Call, Storage, Inherent},
		Indices: pallet_indices::{Module, Call, Storage, Config<T>, Event<T>},
		Balances: pallet_balances::{Module, Call, Storage, Config<T>, Event<T>},
		TransactionPayment: pallet_transaction_payment::{Module, Storage},
		Staking: pallet_staking::{Module, Call, Config<T>, Storage, Event<T>, ValidateUnsigned},
		Session: pallet_session::{Module, Call, Storage, Event, Config<T>},
		Democracy: pallet_democracy::{Module, Call, Storage, Config, Event<T>},
		Council: pallet_collective::<Instance1>::{Module, Call, Storage, Origin<T>, Event<T>, Config<T>},
		TechnicalCommittee: pallet_collective::<Instance2>::{Module, Call, Storage, Origin<T>, Event<T>, Config<T>},
		Elections: pallet_elections_phragmen::{Module, Call, Storage, Event<T>, Config<T>},
		TechnicalMembership: pallet_membership::<Instance1>::{Module, Call, Storage, Event<T>, Config<T>},
		FinalityTracker: pallet_finality_tracker::{Module, Call, Inherent},
		Grandpa: pallet_grandpa::{Module, Call, Storage, Config, Event},
		Treasury: pallet_treasury::{Module, Call, Storage, Config, Event<T>},
		Contracts: pallet_contracts::{Module, Call, Config, Storage, Event<T>},
		Sudo: pallet_sudo::{Module, Call, Config<T>, Storage, Event<T>},
		ImOnline: pallet_im_online::{Module, Call, Storage, Event<T>, ValidateUnsigned, Config<T>},
		AuthorityDiscovery: pallet_authority_discovery::{Module, Call, Config, Storage},
		Offences: pallet_offences::{Module, Call, Storage, Event},
		Historical: pallet_session_historical::{Module},
		RandomnessCollectiveFlip: pallet_randomness_collective_flip::{Module, Call, Storage},
		Identity: pallet_identity::{Module, Call, Storage, Event<T>},
		Society: pallet_society::{Module, Call, Storage, Event<T>, Config<T>},
		Recovery: pallet_recovery::{Module, Call, Storage, Event<T>},
		Vesting: pallet_vesting::{Module, Call, Storage, Event<T>, Config<T>},
		Scheduler: pallet_scheduler::{Module, Call, Storage, Event<T>},
		Register: register::{Module, Call, Storage, Event<T>, Config<T>},
		Report: report::{Module, Call, Storage, Event<T>, Config<T>},
		Mine: mine::{Module, Storage, Call, Event<T>, Config<T>},
		Nicks: pallet_nicks::{Module, Call, Storage, Event<T>},
		TransxCommitee: pallet_collective::<Instance3>::{Module, Call, Storage, Origin<T>, Event<T>, Config<T>},
		GenericAsset: generic_asset::{Module, Storage, Call, Event<T>, Config<T>},
		TxValid: tx_valid::{Module, Call, Storage, Event<T>, ValidateUnsigned},
		AddressValid: address_valid::{Module, Call, Storage, Event<T>, ValidateUnsigned},

	}
);

/// The address format for describing accounts.
pub type Address = <Indices as StaticLookup>::Source;
/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;
/// A Block signed with a Justification
pub type SignedBlock = generic::SignedBlock<Block>;
/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;
/// The SignedExtension to the basic transaction logic.
pub type SignedExtra = (
	frame_system::CheckVersion<Runtime>,
	frame_system::CheckGenesis<Runtime>,
	frame_system::CheckEra<Runtime>,
	frame_system::CheckNonce<Runtime>,
	frame_system::CheckWeight<Runtime>,
	pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
	pallet_grandpa::ValidateEquivocationReport<Runtime>,
);
/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic = generic::UncheckedExtrinsic<Address, Call, Signature, SignedExtra>;
/// The payload being signed in transactions.
pub type SignedPayload = generic::SignedPayload<Call, SignedExtra>;
/// Extrinsic type that has already been checked.
pub type CheckedExtrinsic = generic::CheckedExtrinsic<AccountId, Call, SignedExtra>;
/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<Runtime, Block, frame_system::ChainContext<Runtime>, Runtime, AllModules>;

impl_runtime_apis! {
	impl sp_api::Core<Block> for Runtime {
		fn version() -> RuntimeVersion {
			VERSION
		}

		fn execute_block(block: Block) {
			Executive::execute_block(block)
		}

		fn initialize_block(header: &<Block as BlockT>::Header) {
			Executive::initialize_block(header)
		}
	}

	impl sp_api::Metadata<Block> for Runtime {
		fn metadata() -> OpaqueMetadata {
			Runtime::metadata().into()
		}
	}

	impl sp_block_builder::BlockBuilder<Block> for Runtime {
		fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
			Executive::apply_extrinsic(extrinsic)
		}

		fn finalize_block() -> <Block as BlockT>::Header {
			Executive::finalize_block()
		}

		fn inherent_extrinsics(data: InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
			data.create_extrinsics()
		}

		fn check_inherents(block: Block, data: InherentData) -> CheckInherentsResult {
			data.check_extrinsics(&block)
		}

		fn random_seed() -> <Block as BlockT>::Hash {
			RandomnessCollectiveFlip::random_seed()
		}
	}

	impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
		fn validate_transaction(
			source: TransactionSource,
			tx: <Block as BlockT>::Extrinsic,
		) -> TransactionValidity {
			Executive::validate_transaction(source, tx)
		}
	}

	impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
		fn offchain_worker(header: &<Block as BlockT>::Header) {
			Executive::offchain_worker(header)
		}
	}

	impl fg_primitives::GrandpaApi<Block> for Runtime {
		fn grandpa_authorities() -> GrandpaAuthorityList {
			Grandpa::grandpa_authorities()
		}

		fn submit_report_equivocation_extrinsic(
			equivocation_proof: fg_primitives::EquivocationProof<
				<Block as BlockT>::Hash,
				NumberFor<Block>,
			>,
			key_owner_proof: fg_primitives::OpaqueKeyOwnershipProof,
		) -> Option<()> {
			let key_owner_proof = key_owner_proof.decode()?;

			Grandpa::submit_report_equivocation_extrinsic(
				equivocation_proof,
				key_owner_proof,
			)
		}

		fn generate_key_ownership_proof(
			_set_id: fg_primitives::SetId,
			authority_id: GrandpaId,
		) -> Option<fg_primitives::OpaqueKeyOwnershipProof> {
			use codec::Encode;

			Historical::prove((fg_primitives::KEY_TYPE, authority_id))
				.map(|p| p.encode())
				.map(fg_primitives::OpaqueKeyOwnershipProof::new)
		}
	}

	impl sp_consensus_babe::BabeApi<Block> for Runtime {
		fn configuration() -> sp_consensus_babe::BabeGenesisConfiguration {
			// The choice of `c` parameter (where `1 - c` represents the
			// probability of a slot being empty), is done in accordance to the
			// slot duration and expected target block time, for safely
			// resisting network delays of maximum two seconds.
			// <https://research.web3.foundation/en/latest/polkadot/BABE/Babe/#6-practical-results>
			sp_consensus_babe::BabeGenesisConfiguration {
				slot_duration: Babe::slot_duration(),
				epoch_length: EpochDuration::get(),
				c: PRIMARY_PROBABILITY,
				genesis_authorities: Babe::authorities(),
				randomness: Babe::randomness(),
				allowed_slots: sp_consensus_babe::AllowedSlots::PrimaryAndSecondaryPlainSlots,
			}
		}

		fn current_epoch_start() -> sp_consensus_babe::SlotNumber {
			Babe::current_epoch_start()
		}
	}

	impl sp_authority_discovery::AuthorityDiscoveryApi<Block> for Runtime {
		fn authorities() -> Vec<AuthorityDiscoveryId> {
			AuthorityDiscovery::authorities()
		}
	}

	impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index> for Runtime {
		fn account_nonce(account: AccountId) -> Index {
			System::account_nonce(account)
		}
	}

	impl pallet_contracts_rpc_runtime_api::ContractsApi<Block, AccountId, Balance, BlockNumber>
		for Runtime
	{
		fn call(
			origin: AccountId,
			dest: AccountId,
			value: Balance,
			gas_limit: u64,
			input_data: Vec<u8>,
		) -> ContractExecResult {
			let exec_result =
				Contracts::bare_call(origin, dest.into(), value, gas_limit, input_data);
			match exec_result {
				Ok(v) => ContractExecResult::Success {
					status: v.status,
					data: v.data,
				},
				Err(_) => ContractExecResult::Error,
			}
		}

		fn get_storage(
			address: AccountId,
			key: [u8; 32],
		) -> pallet_contracts_primitives::GetStorageResult {
			Contracts::get_storage(address, key)
		}

		fn rent_projection(
			address: AccountId,
		) -> pallet_contracts_primitives::RentProjectionResult<BlockNumber> {
			Contracts::rent_projection(address)
		}
	}

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<
		Block,
		Balance,
		UncheckedExtrinsic,
	> for Runtime {
		fn query_info(uxt: UncheckedExtrinsic, len: u32) -> RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_info(uxt, len)
		}
	}

	impl sp_session::SessionKeys<Block> for Runtime {
		fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
			SessionKeys::generate(seed)
		}

		fn decode_session_keys(
			encoded: Vec<u8>,
		) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
			SessionKeys::decode_into_raw_public_keys(&encoded)
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	impl frame_benchmarking::Benchmark<Block> for Runtime {
		fn dispatch_benchmark(
			pallet: Vec<u8>,
			benchmark: Vec<u8>,
			lowest_range_values: Vec<u32>,
			highest_range_values: Vec<u32>,
			steps: Vec<u32>,
			repeat: u32,
		) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, sp_runtime::RuntimeString> {
			use frame_benchmarking::{Benchmarking, BenchmarkBatch, add_benchmark};
			// Trying to add benchmarks directly to the Session Pallet caused cyclic dependency issues.
			// To get around that, we separated the Session benchmarks into its own crate, which is why
			// we need these two lines below.
			use pallet_session_benchmarking::Module as SessionBench;
			use pallet_offences_benchmarking::Module as OffencesBench;
			use frame_system_benchmarking::Module as SystemBench;

			impl pallet_session_benchmarking::Trait for Runtime {}
			impl pallet_offences_benchmarking::Trait for Runtime {}
			impl frame_system_benchmarking::Trait for Runtime {}

			let mut batches = Vec::<BenchmarkBatch>::new();
			let params = (&pallet, &benchmark, &lowest_range_values, &highest_range_values, &steps, repeat);

			add_benchmark!(params, batches, b"balances", Balances);
			add_benchmark!(params, batches, b"collective", Council);
			add_benchmark!(params, batches, b"democracy", Democracy);
			add_benchmark!(params, batches, b"identity", Identity);
			add_benchmark!(params, batches, b"im-online", ImOnline);
			add_benchmark!(params, batches, b"offences", OffencesBench::<Runtime>);
			add_benchmark!(params, batches, b"scheduler", Scheduler);
			add_benchmark!(params, batches, b"session", SessionBench::<Runtime>);
			add_benchmark!(params, batches, b"staking", Staking);
			add_benchmark!(params, batches, b"system", SystemBench::<Runtime>);
			add_benchmark!(params, batches, b"timestamp", Timestamp);
			add_benchmark!(params, batches, b"treasury", Treasury);
			add_benchmark!(params, batches, b"utility", Utility);
			add_benchmark!(params, batches, b"vesting", Vesting);

			if batches.is_empty() { return Err("Benchmark not found for this pallet.".into()) }
			Ok(batches)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use frame_system::offchain::CreateSignedTransaction;

	#[test]
	fn validate_transaction_submitter_bounds() {
		fn is_submit_signed_transaction<T>() where
			T: CreateSignedTransaction<Call>,
		{}

		is_submit_signed_transaction::<Runtime>();
	}
}