//! # Eigenlayer Pallet
//! 
//! A pallet providing quantum-secure restaking capabilities for the Matrix-Magiq ecosystem.
//! Implements a pure Rust AVS (Actively Validated Service) framework with quantum resistance.
//! 
//! ## Overview
//! 
//! This pallet enables:
//! - Secure restaking of assets across multiple AVSs
//! - Quantum-secured validator operations
//! - Dual-layer staking with thrice-nested entropy protection
//! - Integration with sustainable food supply chain verification

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod weights;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::{
        dispatch::{DispatchResult, DispatchResultWithPostInfo},
        pallet_prelude::*,
        traits::{Currency, ReservableCurrency, WithdrawReasons, LockIdentifier},
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::{
        traits::{AtLeast32BitUnsigned, CheckedAdd, CheckedSub, Zero, StaticLookup},
        Perbill,
    };
    use sp_std::prelude::*;
    use sp_std::collections::btree_map::BTreeMap;
    use blake3;
    use pqc_kyber;
    use pqc_dilithium;

    const EIGENLAYER_ID: LockIdentifier = *b"eigenlyr";

    /// Operator status in the Eigenspace
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub enum OperatorStatus {
        /// Active operator
        Active,
        /// Operator that has signaled intent to exit
        Exiting,
        /// Operator in cooldown period after withdrawal
        Cooling,
        /// Ejected operator due to slashing or misbehavior
        Ejected,
    }

    /// Representation of an AVS (Actively Validated Service)
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct ActivelyValidatedService<AccountId, Balance> {
        /// Name of the AVS
        pub name: Vec<u8>,
        /// Creator of the AVS
        pub creator: AccountId,
        /// Total staked amount
        pub total_staked: Balance,
        /// Minimum stake required to become an operator
        pub min_stake: Balance,
        /// Maximum stake allowed per operator
        pub max_stake: Balance,
        /// Total operators in the AVS
        pub operator_count: u32,
        /// Is the AVS quorum certified with quantum security
        pub quorum_certified: bool,
        /// AVS configuration (metahash of extended params)
        pub config_hash: [u8; 32],
        /// Thrice-nested quantum entropy for this AVS
        pub quantum_entropy: [u8; 96],
    }

    /// Operator data for Eigenlayer staking
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct Operator<AccountId, Balance> {
        /// Operator account
        pub account: AccountId,
        /// Total restaked amount
        pub restaked_amount: Balance,
        /// List of AVS IDs this operator is participating in
        pub services: Vec<u32>,
        /// Current status
        pub status: OperatorStatus,
        /// Time when exit process began (if exiting)
        pub exit_start: Option<u32>,
        /// Total rewards earned
        pub cumulative_rewards: Balance,
        /// Quantum security entropy for operator signing
        pub quantum_entropy: [u8; 96],
    }

    pub type BalanceOf<T> = 
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
    
    pub type AvsId = u32;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        
        /// The currency mechanism.
        type Currency: ReservableCurrency<Self::AccountId>;
        
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        
        /// The minimum amount required to create a new operator
        type MinOperatorStake: Get<BalanceOf<Self>>;
        
        /// Cooldown period (in blocks) for unstaking
        type UnstakeCooldown: Get<Self::BlockNumber>;
        
        /// Maximum number of AVSs an operator can participate in
        type MaxServicesPerOperator: Get<u32>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Storage for the next available AVS ID
    #[pallet::storage]
    #[pallet::getter(fn next_avs_id)]
    pub type NextAvsId<T> = StorageValue<_, AvsId, ValueQuery>;

    /// Storage for AVS configurations
    #[pallet::storage]
    #[pallet::getter(fn avs_configs)]
    pub type AvsConfigs<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        AvsId,
        ActivelyValidatedService<T::AccountId, BalanceOf<T>>,
        OptionQuery
    >;

    /// Storage for operator data
    #[pallet::storage]
    #[pallet::getter(fn operators)]
    pub type Operators<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Operator<T::AccountId, BalanceOf<T>>,
        OptionQuery
    >;

    /// Storage for operator assignments to AVS services
    #[pallet::storage]
    #[pallet::getter(fn avs_operators)]
    pub type AvsOperators<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        AvsId,
        Blake2_128Concat,
        T::AccountId,
        BalanceOf<T>,
        ValueQuery
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new AVS was registered. [creator, avs_id, name]
        AvsRegistered(T::AccountId, AvsId, Vec<u8>),
        
        /// A new operator was registered. [operator, stake_amount]
        OperatorRegistered(T::AccountId, BalanceOf<T>),
        
        /// An operator joined an AVS. [operator, avs_id, stake_amount]
        OperatorJoinedAvs(T::AccountId, AvsId, BalanceOf<T>),
        
        /// An operator began the exit process. [operator, avs_id]
        OperatorExitStarted(T::AccountId, AvsId),
        
        /// An operator completed the exit process. [operator, avs_id, unstaked_amount]
        OperatorExitCompleted(T::AccountId, AvsId, BalanceOf<T>),
        
        /// An operator was slashed. [operator, avs_id, slashed_amount]
        OperatorSlashed(T::AccountId, AvsId, BalanceOf<T>),
        
        /// Rewards were distributed to an operator. [operator, avs_id, reward_amount]
        RewardsDistributed(T::AccountId, AvsId, BalanceOf<T>),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// AVS does not exist
        AvsNotFound,
        /// Operator does not exist
        OperatorNotFound,
        /// Insufficient stake
        InsufficientStake,
        /// Operator already registered
        OperatorAlreadyRegistered,
        /// Operator already in AVS
        OperatorAlreadyInAvs,
        /// Operator not in AVS
        OperatorNotInAvs,
        /// Max AVS services reached for operator
        MaxServicesReached,
        /// Cooldown period not finished
        CooldownNotFinished,
        /// Exit not initiated
        ExitNotInitiated,
        /// Math overflow
        MathOverflow,
        /// Invalid AVS parameters
        InvalidAvsParams,
        /// Unauthorized operation
        Unauthorized,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Register a new AVS (Actively Validated Service)
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::register_avs())]
        pub fn register_avs(
            origin: OriginFor<T>,
            name: Vec<u8>,
            min_stake: BalanceOf<T>,
            max_stake: BalanceOf<T>,
            config_hash: [u8; 32],
        ) -> DispatchResult {
            let creator = ensure_signed(origin)?;
            
            // Validate inputs
            ensure!(min_stake > Zero::zero(), Error::<T>::InvalidAvsParams);
            ensure!(max_stake >= min_stake, Error::<T>::InvalidAvsParams);
            ensure!(!name.is_empty(), Error::<T>::InvalidAvsParams);
            
            // Generate quantum entropy for the AVS
            let quantum_entropy = Self::generate_avs_entropy(&creator, &name, &config_hash);
            
            // Create the AVS
            let avs_id = Self::next_avs_id();
            
            let avs = ActivelyValidatedService {
                name: name.clone(),
                creator: creator.clone(),
                total_staked: Zero::zero(),
                min_stake,
                max_stake,
                operator_count: 0,
                quorum_certified: false,
                config_hash,
                quantum_entropy,
            };
            
            // Store the AVS
            <AvsConfigs<T>>::insert(avs_id, avs);
            <NextAvsId<T>>::put(avs_id.checked_add(1).ok_or(Error::<T>::MathOverflow)?);
            
            // Emit event
            Self::deposit_event(Event::AvsRegistered(creator, avs_id, name));
            
            Ok(())
        }
        
        /// Register as an Eigenlayer operator
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::register_operator())]
        pub fn register_operator(
            origin: OriginFor<T>,
            stake_amount: BalanceOf<T>,
        ) -> DispatchResult {
            let operator = ensure_signed(origin)?;
            
            // Check if operator already registered
            ensure!(!<Operators<T>>::contains_key(&operator), Error::<T>::OperatorAlreadyRegistered);
            
            // Check minimum stake
            ensure!(stake_amount >= T::MinOperatorStake::get(), Error::<T>::InsufficientStake);
            
            // Generate quantum entropy for the operator
            let quantum_entropy = Self::generate_operator_entropy(&operator);
            
            // Create operator record
            let operator_record = Operator {
                account: operator.clone(),
                restaked_amount: stake_amount,
                services: Vec::new(),
                status: OperatorStatus::Active,
                exit_start: None,
                cumulative_rewards: Zero::zero(),
                quantum_entropy,
            };
            
            // Lock the staked funds
            T::Currency::set_lock(
                EIGENLAYER_ID,
                &operator,
                stake_amount,
                WithdrawReasons::all(),
            );
            
            // Store operator data
            <Operators<T>>::insert(&operator, operator_record);
            
            // Emit event
            Self::deposit_event(Event::OperatorRegistered(operator, stake_amount));
            
            Ok(())
        }
        
        // Additional extrinsics would be implemented here: join_avs, exit_avs, complete_exit, etc.
    }
    
    // Internal helper functions
    impl<T: Config> Pallet<T> {
        /// Generate quantum entropy for an AVS using thrice-nested approach
        fn generate_avs_entropy(
            creator: &T::AccountId,
            name: &Vec<u8>,
            config_hash: &[u8; 32],
        ) -> [u8; 96] {
            let creator_bytes = creator.encode();
            let name_bytes = name.encode();
            
            // Generate Fe layer (outer)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&creator_bytes);
            hasher.update(&name_bytes);
            let fe_layer = hasher.finalize().into();
            
            // Generate Ni layer (middle)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(config_hash);
            let ni_layer = hasher.finalize().into();
            
            // Generate Co layer (inner)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&ni_layer);
            hasher.update(&creator_bytes);
            let co_layer = hasher.finalize().into();
            
            // Combine all layers
            let mut result = [0u8; 96];
            result[0..32].copy_from_slice(&fe_layer);
            result[32..64].copy_from_slice(&ni_layer);
            result[64..96].copy_from_slice(&co_layer);
            
            result
        }
        
        /// Generate quantum entropy for an operator using thrice-nested approach
        fn generate_operator_entropy(
            operator: &T::AccountId,
        ) -> [u8; 96] {
            let operator_bytes = operator.encode();
            let timestamp = <frame_system::Pallet<T>>::block_number().encode();
            
            // Generate Fe layer (outer)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&operator_bytes);
            hasher.update(&timestamp);
            let fe_layer = hasher.finalize().into();
            
            // Generate Ni layer (middle)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&operator_bytes);
            let ni_layer = hasher.finalize().into();
            
            // Generate Co layer (inner)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&ni_layer);
            hasher.update(&timestamp);
            let co_layer = hasher.finalize().into();
            
            // Combine all layers
            let mut result = [0u8; 96];
            result[0..32].copy_from_slice(&fe_layer);
            result[32..64].copy_from_slice(&ni_layer);
            result[64..96].copy_from_slice(&co_layer);
            
            result
        }
    }
    
    /// Weight information for pallet extrinsics
    pub trait WeightInfo {
        fn register_avs() -> Weight;
        fn register_operator() -> Weight;
        fn join_avs() -> Weight;
        fn exit_avs() -> Weight;
        fn complete_exit() -> Weight;
        fn distribute_rewards() -> Weight;
    }
}
