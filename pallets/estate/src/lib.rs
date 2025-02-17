// This file is part of Bit.Country.

// Copyright (C) 2020-2021 Bit.Country.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::pallet_prelude::*;
use frame_support::{dispatch::DispatchResult, ensure, traits::Get, PalletId};
use frame_system::pallet_prelude::*;
use frame_system::{ensure_root, ensure_signed};
use scale_info::TypeInfo;
use sp_runtime::{
	traits::{AccountIdConversion, One, Saturating},
	DispatchError,
};
use sp_std::vec::Vec;

use auction_manager::{Auction, CheckAuctionItemHandler};
use bc_primitives::*;
pub use pallet::*;
use primitives::{
	estate::Estate, EstateId, ItemId, MetaverseId, UndeployedLandBlock, UndeployedLandBlockId, UndeployedLandBlockType,
};
pub use rate::{MintingRateInfo, Range};
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

#[cfg(test)]
mod mock;
mod rate;

#[cfg(test)]
mod tests;

pub mod weights;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::traits::{Currency, Imbalance, ReservableCurrency};
	use sp_runtime::traits::{CheckedAdd, CheckedSub, Zero};

	use primitives::staking::{Bond, RoundInfo, StakeSnapshot};
	use primitives::{RoundIndex, UndeployedLandBlockId};

	use crate::rate::{round_issuance_range, MintingRateInfo};

	use super::*;

	#[pallet::pallet]
	#[pallet::generate_store(trait Store)]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		#[pallet::constant]
		type LandTreasury: Get<PalletId>;
		/// Source of Bit Country Info
		type MetaverseInfoSource: MetaverseTrait<Self::AccountId>;
		/// Currency
		type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
		/// Minimum Land Price
		type MinimumLandPrice: Get<BalanceOf<Self>>;
		/// Council origin which allows to update max bound
		type CouncilOrigin: EnsureOrigin<Self::Origin>;
		/// Auction Handler
		type AuctionHandler: Auction<Self::AccountId, Self::BlockNumber> + CheckAuctionItemHandler;
		#[pallet::constant]
		type MinBlocksPerRound: Get<u32>;
		/// Weight implementation for estate extrinsics
		type WeightInfo: WeightInfo;
		#[pallet::constant]
		type MinimumStake: Get<BalanceOf<Self>>;
		#[pallet::constant]
		type RewardPaymentDelay: Get<u32>;
	}

	type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	/// Get max bound
	#[pallet::storage]
	#[pallet::getter(fn get_max_bounds)]
	pub type MaxBounds<T: Config> = StorageMap<_, Blake2_128Concat, MetaverseId, (i32, i32), ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn all_land_units_count)]
	pub(super) type AllLandUnitsCount<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn all_undeployed_land_unit)]
	pub(super) type TotalUndeployedLandUnit<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn get_land_units)]
	pub type LandUnits<T: Config> =
		StorageDoubleMap<_, Twox64Concat, MetaverseId, Twox64Concat, (i32, i32), T::AccountId, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn next_estate_id)]
	pub type NextEstateId<T: Config> = StorageValue<_, EstateId, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn all_estates_count)]
	pub(super) type AllEstatesCount<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn get_estates)]
	pub(super) type Estates<T: Config> = StorageMap<_, Twox64Concat, EstateId, Vec<(i32, i32)>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn get_estate_owner)]
	pub type EstateOwner<T: Config> =
		StorageDoubleMap<_, Twox64Concat, T::AccountId, Twox64Concat, EstateId, (), OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn next_undeployed_land_block_id)]
	pub(super) type NextUndeployedLandBlockId<T: Config> = StorageValue<_, UndeployedLandBlockId, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn get_undeployed_land_block)]
	pub(super) type UndeployedLandBlocks<T: Config> =
		StorageMap<_, Blake2_128Concat, UndeployedLandBlockId, UndeployedLandBlock<T::AccountId>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn get_undeployed_land_block_owner)]
	pub type UndeployedLandBlocksOwner<T: Config> =
		StorageDoubleMap<_, Twox64Concat, T::AccountId, Twox64Concat, UndeployedLandBlockId, (), OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn round)]
	/// Current round index and next round scheduled transition
	pub type Round<T: Config> = StorageValue<_, RoundInfo<T::BlockNumber>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn minting_rate_config)]
	/// Minting rate configuration
	pub type MintingRateConfig<T: Config> = StorageValue<_, MintingRateInfo, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn total_stake)]
	/// Total NEER locked by estate
	type TotalStake<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn staked)]
	/// Total backing stake for selected candidates in the round
	pub type Staked<T: Config> = StorageMap<_, Twox64Concat, RoundIndex, BalanceOf<T>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn exit_queue)]
	/// A queue of account awaiting exit
	type ExitQueue<T: Config> =
		StorageDoubleMap<_, Twox64Concat, T::AccountId, Twox64Concat, EstateId, (), OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn at_stake)]
	/// Snapshot of estate staking per session
	pub type AtStake<T: Config> = StorageDoubleMap<
		_,
		Twox64Concat,
		RoundIndex,
		Twox64Concat,
		EstateId,
		StakeSnapshot<T::AccountId, BalanceOf<T>>,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn estate_stake)]
	/// Estate staking
	pub type EstateStake<T: Config> =
		StorageDoubleMap<_, Twox64Concat, EstateId, Twox64Concat, T::AccountId, BalanceOf<T>, ValueQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig {
		pub minting_rate_config: MintingRateInfo,
	}

	#[cfg(feature = "std")]
	impl Default for GenesisConfig {
		fn default() -> Self {
			GenesisConfig {
				minting_rate_config: Default::default(),
			}
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig {
		fn build(&self) {
			<MintingRateConfig<T>>::put(self.minting_rate_config.clone());

			// Start Round 1 at Block 0
			let round: RoundInfo<T::BlockNumber> = RoundInfo::new(1u32, 0u32.into(), T::MinBlocksPerRound::get());

			let round_issuance_per_round = round_issuance_range::<T>(self.minting_rate_config.clone());

			<Round<T>>::put(round);
			<Pallet<T>>::deposit_event(Event::NewRound(
				T::BlockNumber::zero(),
				1u32,
				round_issuance_per_round.max,
			));
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub (crate) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Beneficial Account Id, Metaverse Id, Coordinates
		NewLandsMinted(T::AccountId, MetaverseId, Vec<(i32, i32)>),
		/// Metaverse Id, Coordinates, From Account Id, To Account Id
		TransferredLandUnit(MetaverseId, (i32, i32), T::AccountId, T::AccountId),
		/// Estate Id, From Account Id, To Account Id
		TransferredEstate(EstateId, T::AccountId, T::AccountId),
		/// Beneficial Account Id, Metaverse Id, Coordinates
		NewLandUnitMinted(T::AccountId, MetaverseId, (i32, i32)),
		/// Estate Id, Beneficial Account Id, Metaverse Id, Coordinates
		NewEstateMinted(EstateId, T::AccountId, MetaverseId, Vec<(i32, i32)>),
		/// Metaverse Id, Min and Max Coordinate
		MaxBoundSet(MetaverseId, (i32, i32)),
		/// From Account Id, Metaverse Id, Undeployed Land Block Id, Coordinates
		LandBlockDeployed(T::AccountId, MetaverseId, UndeployedLandBlockId, Vec<(i32, i32)>),
		/// Beneficial Account Id, Undeployed Land Block Id
		UndeployedLandBlockIssued(T::AccountId, UndeployedLandBlockId),
		/// From Account Id, To Account Id, Undeployed Land Block Id
		UndeployedLandBlockTransferred(T::AccountId, T::AccountId, UndeployedLandBlockId),
		/// Owner Account Id, Approved Account Id, Undeployed Land Block Id
		UndeployedLandBlockApproved(T::AccountId, T::AccountId, UndeployedLandBlockId),
		/// Owner Account Id, Estate Id
		EstateDestroyed(EstateId, T::AccountId),
		/// Estate Id, Owner Account Id, Coordinates
		EstateUpdated(EstateId, T::AccountId, Vec<(i32, i32)>),
		/// Estate Id, Owner Account Id, Coordinates
		LandUnitAdded(EstateId, T::AccountId, Vec<(i32, i32)>),
		/// Estate Id, Owner Account Id, Coordinates
		LandUnitsRemoved(EstateId, T::AccountId, Vec<(i32, i32)>),
		/// Undeployed Land Block Id
		UndeployedLandBlockUnapproved(UndeployedLandBlockId),
		/// Undeployed Land Block Id
		UndeployedLandBlockFreezed(UndeployedLandBlockId),
		/// Undeployed Land Block Id
		UndeployedLandBlockUnfreezed(UndeployedLandBlockId),
		/// Undeployed Land Block Id
		UndeployedLandBlockBurnt(UndeployedLandBlockId),
		/// Starting Block, Round, Total Land Unit
		NewRound(T::BlockNumber, RoundIndex, u64),
		StakeSnapshotUpdated(RoundIndex, BalanceOf<T>),
		StakersPaid(RoundIndex),
		ExitQueueCleared(RoundIndex),
		/// Owner Account Id, Estate Id, Balance
		EstateStakeIncreased(T::AccountId, EstateId, BalanceOf<T>),
		/// Owner Account Id, Estate Id, Balance
		EstateStakeDecreased(T::AccountId, EstateId, BalanceOf<T>),
		/// Owner Account Id, Estate Id
		EstateStakeLeft(T::AccountId, EstateId),
		/// Account Id, Balance
		StakingRewarded(T::AccountId, BalanceOf<T>),
	}

	#[pallet::error]
	pub enum Error<T> {
		// No permission
		NoPermission,
		// NoAvailableLandId,
		NoAvailableEstateId,
		// Insufficient fund
		InsufficientFund,
		// Estate id already exist
		EstateIdAlreadyExist,
		// Land unit is not available
		LandUnitIsNotAvailable,
		// Land unit is out of bound
		LandUnitIsOutOfBound,
		// No max bound set
		NoMaxBoundSet,
		UndeployedLandBlockNotFound,
		UndeployedLandBlockIsNotTransferable,
		UndeployedLandBlockDoesNotHaveEnoughLandUnits,
		AlreadyOwnTheUndeployedLandBlock,
		UndeployedLandBlockFreezed,
		UndeployedLandBlockAlreadyFreezed,
		UndeployedLandBlockNotFrozen,
		AlreadyOwnTheEstate,
		AlreadyOwnTheLandUnit,
		EstateNotInAuction,
		LandUnitNotInAuction,
		EstateAlreadyInAuction,
		LandUnitAlreadyInAuction,
		EstateDoesNotExist,
		LandUnitDoesNotExist,
		OnlyFrozenUndeployedLandBlockCanBeDestroyed,
		BelowMinimumStake,
		Overflow,
		EstateStakeAlreadyLeft,
		AccountHasNoStake,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(T::WeightInfo::set_max_bounds())]
		pub fn set_max_bounds(
			origin: OriginFor<T>,
			metaverse_id: MetaverseId,
			new_bound: (i32, i32),
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			MaxBounds::<T>::insert(metaverse_id, new_bound);

			Self::deposit_event(Event::<T>::MaxBoundSet(metaverse_id, new_bound));

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::mint_land())]
		pub fn mint_land(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			metaverse_id: MetaverseId,
			coordinate: (i32, i32),
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			// Mint land unit
			Self::mint_land_unit(metaverse_id, &beneficiary, coordinate, false)?;

			// Update total land count
			Self::set_total_land_unit(One::one(), false)?;

			// Update land units
			LandUnits::<T>::insert(metaverse_id, coordinate, beneficiary.clone());

			Self::deposit_event(Event::<T>::NewLandUnitMinted(
				beneficiary.clone(),
				metaverse_id,
				coordinate,
			));

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::mint_lands())]
		pub fn mint_lands(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			metaverse_id: MetaverseId,
			coordinates: Vec<(i32, i32)>,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			// Mint land units
			for coordinate in coordinates.clone() {
				Self::mint_land_unit(metaverse_id, &beneficiary, coordinate, false)?;
			}

			// Update total land count
			Self::set_total_land_unit(coordinates.len() as u64, false)?;

			Self::deposit_event(Event::<T>::NewLandsMinted(
				beneficiary.clone(),
				metaverse_id.clone(),
				coordinates.clone(),
			));

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::transfer_land())]
		pub fn transfer_land(
			origin: OriginFor<T>,
			to: T::AccountId,
			metaverse_id: MetaverseId,
			coordinate: (i32, i32),
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			ensure!(
				!T::AuctionHandler::check_item_in_auction(ItemId::LandUnit(coordinate, metaverse_id)),
				Error::<T>::LandUnitAlreadyInAuction
			);

			Self::do_transfer_landunit(coordinate, &who, &to, metaverse_id)?;
			Ok(().into())
		}

		/// Mint new estate with no existing land unit
		#[pallet::weight(T::WeightInfo::mint_estate())]
		pub fn mint_estate(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			metaverse_id: MetaverseId,
			coordinates: Vec<(i32, i32)>,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			// Generate new estate id
			let new_estate_id = Self::get_new_estate_id()?;

			// Generate sub account from estate
			let estate_account_id = T::LandTreasury::get().into_sub_account(new_estate_id);

			// Mint land units
			for coordinate in coordinates.clone() {
				Self::mint_land_unit(metaverse_id, &estate_account_id, coordinate, false)?;
			}
			// Update total land count
			Self::set_total_land_unit(coordinates.len() as u64, false)?;

			// Update estate information
			Self::update_estate_information(new_estate_id, metaverse_id, &beneficiary, coordinates)?;
			Ok(().into())
		}

		/// Create new estate from existing land units
		#[pallet::weight(T::WeightInfo::create_estate())]
		pub fn create_estate(
			origin: OriginFor<T>,
			beneficiary: T::AccountId,
			metaverse_id: MetaverseId,
			coordinates: Vec<(i32, i32)>,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			// Generate new estate id
			let new_estate_id = Self::get_new_estate_id()?;

			// Generate sub account from estate
			let estate_account_id: T::AccountId = T::LandTreasury::get().into_sub_account(new_estate_id);

			// Mint land units
			for coordinate in coordinates.clone() {
				Self::mint_land_unit(metaverse_id, &estate_account_id, coordinate, true)?;
			}

			// Update estate information
			Self::update_estate_information(new_estate_id, metaverse_id, &beneficiary, coordinates.clone())?;

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::transfer_estate())]
		pub fn transfer_estate(
			origin: OriginFor<T>,
			to: T::AccountId,
			estate_id: EstateId,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			ensure!(
				!T::AuctionHandler::check_item_in_auction(ItemId::Estate(estate_id)),
				Error::<T>::EstateAlreadyInAuction
			);

			Self::do_transfer_estate(estate_id, &who, &to)?;

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::deploy_land_block())]
		pub fn deploy_land_block(
			origin: OriginFor<T>,
			undeployed_land_block_id: UndeployedLandBlockId,
			metaverse_id: MetaverseId,
			coordinates: Vec<(i32, i32)>,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			UndeployedLandBlocks::<T>::try_mutate_exists(
				&undeployed_land_block_id,
				|undeployed_land_block| -> DispatchResultWithPostInfo {
					let mut undeployed_land_block_record = undeployed_land_block
						.as_mut()
						.ok_or(Error::<T>::UndeployedLandBlockNotFound)?;

					ensure!(
						undeployed_land_block_record.owner == who.clone(),
						Error::<T>::NoPermission
					);

					ensure!(
						undeployed_land_block_record.is_frozen == false,
						Error::<T>::UndeployedLandBlockFreezed
					);

					let land_units_to_mint = coordinates.len() as u32;
					ensure!(
						undeployed_land_block_record.number_land_units > land_units_to_mint,
						Error::<T>::UndeployedLandBlockDoesNotHaveEnoughLandUnits
					);

					// Mint land units
					for coordinate in coordinates.clone() {
						Self::mint_land_unit(metaverse_id, &who, coordinate, false)?;
					}

					// Update total land count
					Self::set_total_land_unit(coordinates.len() as u64, false)?;

					// Update undeployed land block
					if undeployed_land_block_record.number_land_units == land_units_to_mint {
						Self::do_burn_undeployed_land_block(undeployed_land_block_id)?;
					} else {
						undeployed_land_block_record.number_land_units = undeployed_land_block_record
							.number_land_units
							.checked_sub(land_units_to_mint)
							.ok_or("Overflow deduct land units from undeployed land block")?;
					}
					Self::set_total_undeployed_land_unit(land_units_to_mint as u64, true)?;

					Self::deposit_event(Event::<T>::LandBlockDeployed(
						who.clone(),
						metaverse_id,
						undeployed_land_block_id,
						coordinates,
					));

					Ok(().into())
				},
			)
		}

		#[pallet::weight(T::WeightInfo::issue_undeployed_land_blocks())]
		pub fn issue_undeployed_land_blocks(
			who: OriginFor<T>,
			beneficiary: T::AccountId,
			number_of_land_block: u32,
			number_land_units_per_land_block: u32,
			undeployed_land_block_type: UndeployedLandBlockType,
		) -> DispatchResultWithPostInfo {
			ensure_root(who)?;

			Self::do_issue_undeployed_land_blocks(
				&beneficiary,
				number_of_land_block,
				number_land_units_per_land_block,
				undeployed_land_block_type,
			)?;

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::freeze_undeployed_land_blocks())]
		pub fn freeze_undeployed_land_blocks(
			origin: OriginFor<T>,
			undeployed_land_block_id: UndeployedLandBlockId,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			Self::do_freeze_undeployed_land_block(undeployed_land_block_id)?;

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::unfreeze_undeployed_land_blocks())]
		pub fn unfreeze_undeployed_land_blocks(
			origin: OriginFor<T>,
			undeployed_land_block_id: UndeployedLandBlockId,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			UndeployedLandBlocks::<T>::try_mutate_exists(
				&undeployed_land_block_id,
				|undeployed_land_block| -> DispatchResultWithPostInfo {
					let mut undeployed_land_block_record = undeployed_land_block
						.as_mut()
						.ok_or(Error::<T>::UndeployedLandBlockNotFound)?;

					ensure!(
						undeployed_land_block_record.is_frozen == true,
						Error::<T>::UndeployedLandBlockNotFrozen
					);

					undeployed_land_block_record.is_frozen = false;

					Self::deposit_event(Event::<T>::UndeployedLandBlockUnfreezed(undeployed_land_block_id));

					Ok(().into())
				},
			)
		}

		#[pallet::weight(T::WeightInfo::transfer_undeployed_land_blocks())]
		pub fn transfer_undeployed_land_blocks(
			origin: OriginFor<T>,
			to: T::AccountId,
			undeployed_land_block_id: UndeployedLandBlockId,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			Self::do_transfer_undeployed_land_block(&who, &to, undeployed_land_block_id)?;

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::burn_undeployed_land_blocks())]
		pub fn burn_undeployed_land_blocks(
			origin: OriginFor<T>,
			undeployed_land_block_id: UndeployedLandBlockId,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;

			Self::do_burn_undeployed_land_block(undeployed_land_block_id)?;

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::approve_undeployed_land_blocks())]
		pub fn approve_undeployed_land_blocks(
			origin: OriginFor<T>,
			to: T::AccountId,
			undeployed_land_block_id: UndeployedLandBlockId,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			UndeployedLandBlocks::<T>::try_mutate_exists(
				&undeployed_land_block_id,
				|undeployed_land_block| -> DispatchResultWithPostInfo {
					let mut undeployed_land_block_record = undeployed_land_block
						.as_mut()
						.ok_or(Error::<T>::UndeployedLandBlockNotFound)?;

					ensure!(
						undeployed_land_block_record.owner == who.clone(),
						Error::<T>::NoPermission
					);

					ensure!(
						undeployed_land_block_record.is_frozen == false,
						Error::<T>::UndeployedLandBlockAlreadyFreezed
					);

					undeployed_land_block_record.approved = Some(to.clone());

					Self::deposit_event(Event::<T>::UndeployedLandBlockApproved(
						who.clone(),
						to.clone(),
						undeployed_land_block_id.clone(),
					));

					Ok(().into())
				},
			)
		}

		#[pallet::weight(T::WeightInfo::unapprove_undeployed_land_blocks())]
		pub fn unapprove_undeployed_land_blocks(
			origin: OriginFor<T>,
			undeployed_land_block_id: UndeployedLandBlockId,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			UndeployedLandBlocks::<T>::try_mutate_exists(
				&undeployed_land_block_id,
				|undeployed_land_block| -> DispatchResultWithPostInfo {
					let mut undeployed_land_block_record = undeployed_land_block
						.as_mut()
						.ok_or(Error::<T>::UndeployedLandBlockNotFound)?;

					ensure!(
						undeployed_land_block_record.owner == who.clone(),
						Error::<T>::NoPermission
					);

					ensure!(
						undeployed_land_block_record.is_frozen == false,
						Error::<T>::UndeployedLandBlockAlreadyFreezed
					);

					undeployed_land_block_record.approved = None;

					Self::deposit_event(Event::<T>::UndeployedLandBlockUnapproved(
						undeployed_land_block_id.clone(),
					));

					Ok(().into())
				},
			)
		}

		#[pallet::weight(T::WeightInfo::dissolve_estate())]
		pub fn dissolve_estate(
			origin: OriginFor<T>,
			estate_id: EstateId,
			metaverse_id: MetaverseId,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			ensure!(
				!T::AuctionHandler::check_item_in_auction(ItemId::Estate(estate_id)),
				Error::<T>::EstateAlreadyInAuction
			);

			let land_units = Estates::<T>::get(estate_id).ok_or(Error::<T>::EstateDoesNotExist)?;

			EstateOwner::<T>::try_mutate_exists(&who, &estate_id, |estate_by_owner| {
				//ensure there is record of the estate owner with estate id and account id
				ensure!(estate_by_owner.is_some(), Error::<T>::NoPermission);

				// Reset estate ownership
				*estate_by_owner = None;

				// Remove estate
				Estates::<T>::remove(&estate_id);

				// Update total estates
				let total_estates_count = Self::all_estates_count();
				let new_total_estates_count = total_estates_count
					.checked_sub(One::one())
					.ok_or("Overflow adding new count to total estates")?;
				AllEstatesCount::<T>::put(new_total_estates_count);

				// Update land units relationship
				for land_unit in land_units.clone() {
					LandUnits::<T>::try_mutate_exists(
						&metaverse_id,
						&land_unit,
						|maybe_account| -> Result<(), DispatchError> {
							*maybe_account = Some(who.clone());

							Ok(())
						},
					);
				}

				Self::deposit_event(Event::<T>::EstateDestroyed(estate_id.clone(), who.clone()));

				Ok(().into())
			})
		}

		#[pallet::weight(T::WeightInfo::add_land_unit_to_estate())]
		pub fn add_land_unit_to_estate(
			origin: OriginFor<T>,
			estate_id: EstateId,
			metaverse_id: MetaverseId,
			land_units: Vec<(i32, i32)>,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			ensure!(
				!T::AuctionHandler::check_item_in_auction(ItemId::Estate(estate_id)),
				Error::<T>::EstateAlreadyInAuction
			);

			Estates::<T>::get(estate_id).ok_or(Error::<T>::EstateDoesNotExist)?;

			// Check estate ownership
			ensure!(
				Self::get_estate_owner(&who, &estate_id) == Some(()),
				Error::<T>::NoPermission
			);

			// Check land unit ownership
			for land_unit in land_units.clone() {
				ensure!(
					Self::get_land_units(metaverse_id, land_unit) == who,
					Error::<T>::LandUnitDoesNotExist
				);
			}

			// Mutate estates
			Estates::<T>::try_mutate_exists(&estate_id, |maybe_land_units| {
				// Append new coordinates to estate
				let mut land_units_by_estate = maybe_land_units.as_mut().ok_or(Error::<T>::EstateDoesNotExist)?;
				land_units_by_estate.append(&mut land_units.clone());

				// Mutate land unit ownership
				let estate_account_id: T::AccountId = T::LandTreasury::get().into_sub_account(estate_id);

				// Mutate land unit ownership
				for land_unit in land_units.clone() {
					LandUnits::<T>::try_mutate_exists(
						&metaverse_id,
						&land_unit,
						|maybe_account| -> Result<(), DispatchError> {
							*maybe_account = Some(estate_account_id.clone());

							Ok(())
						},
					);
				}

				Self::deposit_event(Event::<T>::LandUnitAdded(
					estate_id.clone(),
					who.clone(),
					land_units.clone(),
				));

				Ok(().into())
			})
		}

		#[pallet::weight(T::WeightInfo::remove_land_unit_from_estate())]
		pub fn remove_land_unit_from_estate(
			origin: OriginFor<T>,
			estate_id: EstateId,
			metaverse_id: MetaverseId,
			land_units: Vec<(i32, i32)>,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			ensure!(
				!T::AuctionHandler::check_item_in_auction(ItemId::Estate(estate_id)),
				Error::<T>::EstateAlreadyInAuction
			);

			Estates::<T>::get(estate_id).ok_or(Error::<T>::EstateDoesNotExist)?;

			// Check estate ownership
			ensure!(
				Self::get_estate_owner(&who, &estate_id) == Some(()),
				Error::<T>::NoPermission
			);

			// Mutate estates
			Estates::<T>::try_mutate_exists(&estate_id, |maybe_land_units| {
				let mut land_units_by_estate = maybe_land_units.as_mut().ok_or(Error::<T>::EstateDoesNotExist)?;

				// Mutate land unit ownership
				for land_unit in land_units.clone() {
					// Remove coordinates from estate
					let index = land_units_by_estate.iter().position(|x| *x == land_unit).unwrap();
					land_units_by_estate.remove(index);

					LandUnits::<T>::try_mutate_exists(
						&metaverse_id,
						&land_unit,
						|maybe_account| -> Result<(), DispatchError> {
							*maybe_account = Some(who.clone());

							Ok(())
						},
					);
				}

				Self::deposit_event(Event::<T>::LandUnitsRemoved(
					estate_id.clone(),
					who.clone(),
					land_units.clone(),
				));

				Ok(().into())
			})
		}

		#[pallet::weight(T::WeightInfo::bond_more())]
		pub fn bond_more(origin: OriginFor<T>, estate_id: EstateId, more: BalanceOf<T>) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			Estates::<T>::get(estate_id).ok_or(Error::<T>::EstateDoesNotExist)?;

			// Check estate ownership
			ensure!(
				Self::get_estate_owner(&who, &estate_id) == Some(()),
				Error::<T>::NoPermission
			);

			// Check exit queue
			ensure!(
				<ExitQueue<T>>::get(&who, estate_id) == None,
				Error::<T>::EstateStakeAlreadyLeft
			);

			// Update EstateStake
			let mut staked_balance = <EstateStake<T>>::get(estate_id, &who);
			let total = staked_balance.checked_add(&more).ok_or(Error::<T>::Overflow)?;

			ensure!(total >= T::MinimumStake::get(), Error::<T>::BelowMinimumStake);

			// Reserve balance
			T::Currency::reserve(&who, more)?;

			<EstateStake<T>>::insert(estate_id, &who, total);

			// Update TotalStake
			let new_total_staked = <TotalStake<T>>::get().saturating_add(more);
			<TotalStake<T>>::put(new_total_staked);

			Self::deposit_event(Event::EstateStakeIncreased(who, estate_id, more));

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::bond_less())]
		pub fn bond_less(origin: OriginFor<T>, estate_id: EstateId, less: BalanceOf<T>) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			Estates::<T>::get(estate_id).ok_or(Error::<T>::EstateDoesNotExist)?;

			// Check estate ownership
			ensure!(
				Self::get_estate_owner(&who, &estate_id) == Some(()),
				Error::<T>::NoPermission
			);

			// Check exit queue
			ensure!(
				<ExitQueue<T>>::get(&who, estate_id) == None,
				Error::<T>::EstateStakeAlreadyLeft
			);

			// Check stake balance
			let mut staked_balance = <EstateStake<T>>::get(estate_id, &who);
			let remaining = staked_balance.checked_sub(&less).ok_or(Error::<T>::Overflow)?;

			ensure!(remaining >= T::MinimumStake::get(), Error::<T>::BelowMinimumStake);

			// Reserve balance
			T::Currency::unreserve(&who, less);

			<EstateStake<T>>::insert(estate_id, &who, remaining);

			// Update TotalStake
			let new_total_staked = <TotalStake<T>>::get().saturating_sub(less);
			<TotalStake<T>>::put(new_total_staked);

			Self::deposit_event(Event::EstateStakeDecreased(who, estate_id, less));

			Ok(().into())
		}

		#[pallet::weight(T::WeightInfo::leave_staking())]
		pub fn leave_staking(origin: OriginFor<T>, estate_id: EstateId) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;

			Estates::<T>::get(estate_id).ok_or(Error::<T>::EstateDoesNotExist)?;

			ensure!(
				<ExitQueue<T>>::get(&who, estate_id) == None,
				Error::<T>::EstateStakeAlreadyLeft
			);

			ensure!(
				<EstateStake<T>>::get(estate_id, &who) > BalanceOf::<T>::zero(),
				Error::<T>::AccountHasNoStake
			);

			<ExitQueue<T>>::insert(&who, estate_id, ());

			Self::deposit_event(Event::EstateStakeLeft(who, estate_id));

			Ok(().into())
		}
	}

	impl<T: Config> Pallet<T> {
		fn pay_stakers(next: RoundIndex) {
			// payout is next - duration rounds ago => next - duration > 0 else return early
			let duration = T::RewardPaymentDelay::get();
			if next <= duration {
				return;
			}
			let round_to_payout = next - duration;

			// issue BIT for rewards distribution
			let total_staked = <Staked<T>>::get(round_to_payout);
			let total_issuance = Self::compute_issuance(total_staked);

			let mut left_issuance = total_issuance;

			// a local fn to transfer rewards to the account specified
			let mint = |amt: BalanceOf<T>, to: T::AccountId| {
				if let Ok(imb) = T::Currency::deposit_into_existing(&to, amt) {
					Self::deposit_event(Event::StakingRewarded(to.clone(), imb.peek()));
				}
			};

			for (estate_id, stake_snapshot) in <AtStake<T>>::drain_prefix(round_to_payout) {
				for Bond { staker: owner, amount } in stake_snapshot.stakers {
					// TODO: TBD on the rewards amount
					let amount_due = amount;
					mint(1u32.into(), owner);
				}
			}
		}

		/// Clear exit queue. return stake to account
		fn clear_exit_queue(now: RoundIndex) {
			for (account_id, estate_id, val) in <ExitQueue<T>>::drain() {
				let staked_amount = <EstateStake<T>>::get(estate_id, &account_id);

				// return stake to account
				T::Currency::unreserve(&account_id, staked_amount);

				<EstateStake<T>>::remove(estate_id, &account_id);
			}
		}

		fn update_stake_snapshot(next: RoundIndex) -> BalanceOf<T> {
			let mut total = BalanceOf::<T>::zero();

			for estate_id in <Estates<T>>::iter_keys() {
				let mut total_bond = BalanceOf::<T>::zero();
				let mut stakers: Vec<Bond<T::AccountId, BalanceOf<T>>> = Vec::new();

				for (account_id, amount) in <EstateStake<T>>::iter_prefix(estate_id) {
					stakers.push(Bond {
						staker: account_id.clone(),
						amount,
					});

					total += amount;
					total_bond += amount;
				}
				if stakers.len() > 0 {
					<AtStake<T>>::insert(next, estate_id, StakeSnapshot { stakers, total_bond });
				}
			}

			<TotalStake<T>>::put(total);
			total
		}

		fn compute_issuance(staked: BalanceOf<T>) -> BalanceOf<T> {
			//TODO: need to decide on how much BIT need to be issued per session
			staked.saturating_add(staked)
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<T::BlockNumber> for Pallet<T> {
		fn on_initialize(n: T::BlockNumber) -> Weight {
			let minting_config = <MintingRateConfig<T>>::get();
			let mut round = <Round<T>>::get();
			if round.should_update(n) {
				// mutate round
				round.update(n);

				let round_issuance_per_round = round_issuance_range::<T>(minting_config);

				//TODO do actual minting new undeployed land block
				let land_register_treasury = T::LandTreasury::get().into_account();

				// Pay all stakers for T::RewardPaymentDelay rounds ago
				Self::pay_stakers(round.current);
				Self::deposit_event(Event::StakersPaid(round.current));

				// Clear exit queue
				Self::clear_exit_queue(round.current);
				Self::deposit_event(Event::ExitQueueCleared(round.current));

				// Update stake snapshot
				let total = Self::update_stake_snapshot(round.current);

				<Round<T>>::put(round);

				<Staked<T>>::insert(round.current, <TotalStake<T>>::get());

				Self::deposit_event(Event::StakeSnapshotUpdated(round.current, total));

				Self::do_issue_undeployed_land_blocks(
					&land_register_treasury,
					round_issuance_per_round.ideal as u32,
					100,
					UndeployedLandBlockType::Transferable,
				);

				Self::deposit_event(Event::NewRound(
					round.first,
					round.current,
					round_issuance_per_round.max,
				));
				<T as pallet::Config>::WeightInfo::active_issue_undeploy_land_block()
			} else {
				0
			}
		}
	}
}

impl<T: Config> Pallet<T> {
	fn get_new_estate_id() -> Result<EstateId, DispatchError> {
		let estate_id = NextEstateId::<T>::try_mutate(|id| -> Result<EstateId, DispatchError> {
			let current_id = *id;
			*id = id.checked_add(One::one()).ok_or(Error::<T>::NoAvailableEstateId)?;
			Ok(current_id)
		})?;
		Ok(estate_id)
	}

	fn mint_land_unit(
		metaverse_id: MetaverseId,
		beneficiary: &T::AccountId,
		coordinate: (i32, i32),
		existing_land_units: bool,
	) -> DispatchResult {
		// Ensure the max bound is set for the bit country
		ensure!(MaxBounds::<T>::contains_key(metaverse_id), Error::<T>::NoMaxBoundSet);

		let max_bound = MaxBounds::<T>::get(metaverse_id);

		if existing_land_units {
			// Check whether the coordinate exists
			ensure!(
				LandUnits::<T>::contains_key(metaverse_id, coordinate),
				Error::<T>::LandUnitIsNotAvailable
			);
		} else {
			ensure!(
				!LandUnits::<T>::contains_key(metaverse_id, coordinate),
				Error::<T>::LandUnitIsNotAvailable
			);
		}

		// Check whether the coordinate is within the bound
		ensure!(
			(coordinate.0 >= max_bound.0 && max_bound.1 >= coordinate.0)
				&& (coordinate.1 >= max_bound.0 && max_bound.1 >= coordinate.1),
			Error::<T>::LandUnitIsOutOfBound
		);

		LandUnits::<T>::insert(metaverse_id, coordinate, beneficiary.clone());
		Ok(())
	}

	fn update_estate_information(
		new_estate_id: EstateId,
		metaverse_id: MetaverseId,
		beneficiary: &T::AccountId,
		coordinates: Vec<(i32, i32)>,
	) -> DispatchResult {
		// Update total estates
		let total_estates_count = Self::all_estates_count();
		let new_total_estates_count = total_estates_count
			.checked_add(One::one())
			.ok_or("Overflow adding new count to total estates")?;
		AllEstatesCount::<T>::put(new_total_estates_count);

		// Update estates
		Estates::<T>::insert(new_estate_id, coordinates.clone());

		EstateOwner::<T>::insert(beneficiary.clone(), new_estate_id, {});

		Self::deposit_event(Event::<T>::NewEstateMinted(
			new_estate_id.clone(),
			beneficiary.clone(),
			metaverse_id,
			coordinates.clone(),
		));

		Ok(())
	}

	fn get_new_undeployed_land_block_id() -> Result<UndeployedLandBlockId, DispatchError> {
		let undeployed_land_block_id =
			NextUndeployedLandBlockId::<T>::try_mutate(|id| -> Result<UndeployedLandBlockId, DispatchError> {
				let current_id = *id;
				*id = id.checked_add(One::one()).ok_or(Error::<T>::NoAvailableEstateId)?;
				Ok(current_id)
			})?;
		Ok(undeployed_land_block_id)
	}

	fn do_transfer_undeployed_land_block(
		who: &T::AccountId,
		to: &T::AccountId,
		undeployed_land_block_id: UndeployedLandBlockId,
	) -> Result<UndeployedLandBlockId, DispatchError> {
		UndeployedLandBlocks::<T>::try_mutate_exists(
			&undeployed_land_block_id,
			|undeployed_land_block| -> Result<UndeployedLandBlockId, DispatchError> {
				let mut undeployed_land_block_record = undeployed_land_block
					.as_mut()
					.ok_or(Error::<T>::UndeployedLandBlockNotFound)?;

				ensure!(
					undeployed_land_block_record.owner == who.clone(),
					Error::<T>::NoPermission
				);

				ensure!(
					undeployed_land_block_record.is_frozen == false,
					Error::<T>::UndeployedLandBlockAlreadyFreezed
				);

				ensure!(
					undeployed_land_block_record.undeployed_land_block_type == UndeployedLandBlockType::Transferable,
					Error::<T>::UndeployedLandBlockIsNotTransferable
				);

				undeployed_land_block_record.owner = to.clone();

				UndeployedLandBlocksOwner::<T>::remove(who.clone(), &undeployed_land_block_id);
				UndeployedLandBlocksOwner::<T>::insert(to.clone(), &undeployed_land_block_id, ());

				Self::deposit_event(Event::<T>::UndeployedLandBlockTransferred(
					who.clone(),
					to.clone(),
					undeployed_land_block_id.clone(),
				));

				Ok(undeployed_land_block_id)
			},
		)
	}

	fn do_burn_undeployed_land_block(
		undeployed_land_block_id: UndeployedLandBlockId,
	) -> Result<UndeployedLandBlockId, DispatchError> {
		let undeployed_land_block_info =
			UndeployedLandBlocks::<T>::get(undeployed_land_block_id).ok_or(Error::<T>::UndeployedLandBlockNotFound)?;

		ensure!(
			undeployed_land_block_info.is_frozen,
			Error::<T>::OnlyFrozenUndeployedLandBlockCanBeDestroyed
		);
		Self::set_total_undeployed_land_unit(undeployed_land_block_info.number_land_units as u64, true)?;
		UndeployedLandBlocksOwner::<T>::remove(undeployed_land_block_info.owner, &undeployed_land_block_id);
		UndeployedLandBlocks::<T>::remove(&undeployed_land_block_id);

		Self::deposit_event(Event::<T>::UndeployedLandBlockBurnt(undeployed_land_block_id.clone()));

		Ok(undeployed_land_block_id)
	}

	fn do_freeze_undeployed_land_block(
		undeployed_land_block_id: UndeployedLandBlockId,
	) -> Result<UndeployedLandBlockId, DispatchError> {
		UndeployedLandBlocks::<T>::try_mutate_exists(
			&undeployed_land_block_id,
			|undeployed_land_block| -> Result<UndeployedLandBlockId, DispatchError> {
				let mut undeployed_land_block_record = undeployed_land_block
					.as_mut()
					.ok_or(Error::<T>::UndeployedLandBlockNotFound)?;

				ensure!(
					undeployed_land_block_record.is_frozen == false,
					Error::<T>::UndeployedLandBlockAlreadyFreezed
				);

				undeployed_land_block_record.is_frozen = true;

				Self::deposit_event(Event::<T>::UndeployedLandBlockFreezed(undeployed_land_block_id));

				Ok(undeployed_land_block_id)
			},
		)
	}

	fn do_issue_undeployed_land_blocks(
		beneficiary: &T::AccountId,
		number_of_land_block: u32,
		number_land_units_per_land_block: u32,
		undeployed_land_block_type: UndeployedLandBlockType,
	) -> Result<Vec<UndeployedLandBlockId>, DispatchError> {
		let mut undeployed_land_block_ids: Vec<UndeployedLandBlockId> = Vec::new();

		for _ in 0..number_of_land_block {
			let new_undeployed_land_block_id = Self::get_new_undeployed_land_block_id()?;

			let undeployed_land_block = UndeployedLandBlock {
				id: new_undeployed_land_block_id,
				number_land_units: number_land_units_per_land_block,
				undeployed_land_block_type,
				approved: None,
				is_frozen: false,
				owner: beneficiary.clone(),
			};

			UndeployedLandBlocks::<T>::insert(new_undeployed_land_block_id, undeployed_land_block);

			UndeployedLandBlocksOwner::<T>::insert(beneficiary.clone(), new_undeployed_land_block_id, ());

			// Update total undeployed land  count
			Self::set_total_undeployed_land_unit(number_land_units_per_land_block as u64, false)?;

			Self::deposit_event(Event::<T>::UndeployedLandBlockIssued(
				beneficiary.clone(),
				new_undeployed_land_block_id.clone(),
			));

			undeployed_land_block_ids.push(new_undeployed_land_block_id);
		}

		Ok(undeployed_land_block_ids)
	}

	fn do_transfer_estate(
		estate_id: EstateId,
		from: &T::AccountId,
		to: &T::AccountId,
	) -> Result<EstateId, DispatchError> {
		EstateOwner::<T>::try_mutate_exists(
			&from,
			&estate_id,
			|estate_by_owner| -> Result<EstateId, DispatchError> {
				//ensure there is record of the estate owner with estate id and account id
				ensure!(estate_by_owner.is_some(), Error::<T>::NoPermission);

				ensure!(from != to, Error::<T>::AlreadyOwnTheEstate);

				*estate_by_owner = None;
				EstateOwner::<T>::insert(to.clone(), estate_id.clone(), ());

				Self::deposit_event(Event::<T>::TransferredEstate(
					estate_id.clone(),
					from.clone(),
					to.clone(),
				));

				Ok(estate_id)
			},
		)
	}

	fn do_transfer_landunit(
		coordinate: (i32, i32),
		from: &T::AccountId,
		to: &T::AccountId,
		metaverse_id: MetaverseId,
	) -> Result<(i32, i32), DispatchError> {
		LandUnits::<T>::try_mutate_exists(
			&metaverse_id,
			&coordinate,
			|land_unit_owner| -> Result<(i32, i32), DispatchError> {
				// ensure there is record of the land unit with bit country id and coordinate
				ensure!(land_unit_owner.is_some(), Error::<T>::NoPermission);

				// Check ownership
				let owner = land_unit_owner.as_ref().map(|t| t);
				ensure!(owner == Some(&from), Error::<T>::NoPermission);

				ensure!(from != to, Error::<T>::AlreadyOwnTheLandUnit);

				*land_unit_owner = None;
				LandUnits::<T>::insert(metaverse_id.clone(), coordinate.clone(), to.clone());

				// Update
				Self::deposit_event(Event::<T>::TransferredLandUnit(
					metaverse_id.clone(),
					coordinate.clone(),
					from.clone(),
					to.clone(),
				));

				Ok(coordinate)
			},
		)
	}

	fn set_total_undeployed_land_unit(total: u64, deduct: bool) -> Result<(), DispatchError> {
		let total_undeployed_land_units = Self::all_undeployed_land_unit();

		if deduct {
			let new_total_undeployed_land_unit_count = total_undeployed_land_units
				.checked_sub(total)
				.ok_or("Overflow deducting new count to total undeployed lands")?;
			TotalUndeployedLandUnit::<T>::put(new_total_undeployed_land_unit_count);
		} else {
			let new_total_undeployed_land_unit_count = total_undeployed_land_units
				.checked_add(total)
				.ok_or("Overflow adding new count to total undeployed lands")?;
			TotalUndeployedLandUnit::<T>::put(new_total_undeployed_land_unit_count);
		}

		Ok(())
	}

	fn set_total_land_unit(total: u64, deduct: bool) -> Result<(), DispatchError> {
		let total_land_units_count = Self::all_land_units_count();

		if deduct {
			let new_total_land_units_count = total_land_units_count
				.checked_sub(total)
				.ok_or("Overflow deducting new count to total lands")?;
			AllLandUnitsCount::<T>::put(new_total_land_units_count);
		} else {
			let new_total_land_units_count = total_land_units_count
				.checked_add(total)
				.ok_or("Overflow adding new count to total lands")?;
			AllLandUnitsCount::<T>::put(new_total_land_units_count);
		}
		Ok(())
	}
}

impl<T: Config> MetaverseLandTrait<T::AccountId> for Pallet<T> {
	fn get_user_land_units(who: &T::AccountId, metaverse_id: &MetaverseId) -> Vec<(i32, i32)> {
		// Check land units owner.
		let mut total_land_units: Vec<(i32, i32)> = Vec::default();

		let land_in_metaverse = LandUnits::<T>::iter_prefix(metaverse_id)
			.filter(|(_, owner)| owner == who)
			.collect::<Vec<_>>();

		for land_unit in land_in_metaverse {
			let land = land_unit.0;
			total_land_units.push(land);
		}

		let estate_ids_by_owner: Vec<EstateId> =
			EstateOwner::<T>::iter_prefix(who).map(|res| res.0).collect::<Vec<_>>();

		for estate_id in estate_ids_by_owner {
			let mut coordinates = Estates::<T>::get(&estate_id).unwrap();
			total_land_units.append(&mut coordinates)
		}

		total_land_units
	}

	fn is_user_own_metaverse_land(who: &T::AccountId, metaverse_id: &MetaverseId) -> bool {
		Self::get_user_land_units(&who, metaverse_id).len() > 0
	}
}

impl<T: Config> UndeployedLandBlocksTrait<T::AccountId> for Pallet<T> {
	fn issue_undeployed_land_blocks(
		beneficiary: &T::AccountId,
		number_of_land_block: u32,
		number_land_units_per_land_block: u32,
		undeployed_land_block_type: UndeployedLandBlockType,
	) -> Result<Vec<UndeployedLandBlockId>, DispatchError> {
		let new_undeployed_land_block_id = Self::do_issue_undeployed_land_blocks(
			&beneficiary,
			number_of_land_block,
			number_land_units_per_land_block,
			undeployed_land_block_type,
		)?;

		Ok(new_undeployed_land_block_id)
	}

	fn transfer_undeployed_land_block(
		who: &T::AccountId,
		to: &T::AccountId,
		undeployed_land_block_id: UndeployedLandBlockId,
	) -> Result<UndeployedLandBlockId, DispatchError> {
		let undeployed_land_block_id = Self::do_transfer_undeployed_land_block(who, to, undeployed_land_block_id)?;

		Ok(undeployed_land_block_id)
	}

	fn burn_undeployed_land_block(
		undeployed_land_block_id: UndeployedLandBlockId,
	) -> Result<UndeployedLandBlockId, DispatchError> {
		let undeployed_land_block_id = Self::do_burn_undeployed_land_block(undeployed_land_block_id)?;

		Ok(undeployed_land_block_id)
	}

	fn freeze_undeployed_land_block(
		undeployed_land_block_id: UndeployedLandBlockId,
	) -> Result<UndeployedLandBlockId, DispatchError> {
		let undeployed_land_block_id = Self::do_freeze_undeployed_land_block(undeployed_land_block_id)?;

		Ok(undeployed_land_block_id)
	}
}

impl<T: Config> Estate<T::AccountId> for Pallet<T> {
	fn transfer_estate(estate_id: EstateId, from: &T::AccountId, to: &T::AccountId) -> Result<EstateId, DispatchError> {
		ensure!(
			T::AuctionHandler::check_item_in_auction(ItemId::Estate(estate_id)),
			Error::<T>::EstateNotInAuction
		);

		let estate_id = Self::do_transfer_estate(estate_id, from, to)?;
		Ok(estate_id)
	}

	fn transfer_landunit(
		coordinate: (i32, i32),
		from: &T::AccountId,
		to: &(T::AccountId, MetaverseId),
	) -> Result<(i32, i32), DispatchError> {
		ensure!(
			T::AuctionHandler::check_item_in_auction(ItemId::LandUnit(coordinate, to.1)),
			Error::<T>::LandUnitNotInAuction
		);

		let coordinate = Self::do_transfer_landunit(coordinate, from, &(to).0, to.1)?;
		Ok(coordinate)
	}

	fn check_estate(estate_id: EstateId) -> Result<bool, DispatchError> {
		Ok(Estates::<T>::contains_key(estate_id))
	}

	fn check_landunit(metaverse_id: MetaverseId, coordinate: (i32, i32)) -> Result<bool, DispatchError> {
		Ok(LandUnits::<T>::contains_key(metaverse_id, coordinate))
	}

	fn get_total_land_units() -> u64 {
		AllLandUnitsCount::<T>::get()
	}

	fn get_total_undeploy_land_units() -> u64 {
		TotalUndeployedLandUnit::<T>::get()
	}
}
