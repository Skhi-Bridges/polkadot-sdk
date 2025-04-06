//! # RPC-Q999 Pallet
//! 
//! Quantum-secure token standard with Actorx circuit profiles, zones, and wallets.
//! Replaces the previous ERC-Q999 standard with enhanced features.

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
        dispatch::DispatchResult,
        pallet_prelude::*,
        traits::{Currency, ReservableCurrency, ExistenceRequirement},
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::{
        traits::{AtLeast32BitUnsigned, CheckedAdd, CheckedSub, Zero},
        ArithmeticError,
    };
    use sp_std::prelude::*;
    use sp_std::collections::btree_map::BTreeMap;
    use blake3;
    use pqc_kyber;
    use pqc_dilithium;

    pub type TokenId = u128;
    pub type ZoneId = u32;
    pub type ProfileId = u32;
    pub type CircuitId = u32;
    
    pub type BalanceOf<T> = 
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    /// Actorx quantum circuit structure
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct ActorxCircuit {
        /// Circuit identifier
        pub id: CircuitId,
        /// Circuit name
        pub name: Vec<u8>,
        /// Circuit creator
        pub creator: Vec<u8>,
        /// Circuit description
        pub description: Vec<u8>,
        /// Circuit type identifiers
        pub circuit_type: Vec<u8>,
        /// Quantum gate sequence (serialized)
        pub gate_sequence: Vec<u8>,
        /// Qubit count
        pub qubit_count: u8,
        /// Thrice-nested entropy
        pub entropy: [u8; 96],
    }

    /// Actorx profile data
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct ActorxProfile<AccountId, Balance> {
        /// Profile identifier
        pub id: ProfileId,
        /// Profile owner
        pub owner: AccountId,
        /// Profile name
        pub name: Vec<u8>,
        /// Associated circuits
        pub circuits: Vec<CircuitId>,
        /// Associated zones
        pub zones: Vec<ZoneId>,
        /// Profile metadata (IPFS CID)
        pub metadata: Vec<u8>,
        /// Social credentials
        pub social_identities: Vec<Vec<u8>>,
        /// Staked balance
        pub staked: Balance,
        /// Thrice-nested entropy
        pub entropy: [u8; 96],
    }

    /// Actorx zone data
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct ActorxZone<AccountId, Balance> {
        /// Zone identifier
        pub id: ZoneId,
        /// Zone controller
        pub controller: AccountId,
        /// Zone name
        pub name: Vec<u8>,
        /// Zone description
        pub description: Vec<u8>,
        /// Permitted circuits
        pub permitted_circuits: Vec<CircuitId>,
        /// Member profiles
        pub members: Vec<ProfileId>,
        /// Total staked balance
        pub total_staked: Balance,
        /// Thrice-nested entropy
        pub entropy: [u8; 96],
    }

    /// RPC-Q999 token data
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct RPCQ999Token<AccountId, Balance> {
        /// Token identifier
        pub id: TokenId,
        /// Token owner
        pub owner: AccountId,
        /// Token name
        pub name: Vec<u8>,
        /// Token symbol
        pub symbol: Vec<u8>,
        /// Token metadata (IPFS CID)
        pub metadata: Vec<u8>,
        /// Associated circuit
        pub circuit: Option<CircuitId>,
        /// Token balance
        pub balance: Balance,
        /// Required zones for transfer
        pub required_zones: Vec<ZoneId>,
        /// Minimum profile level
        pub min_profile_level: u8,
        /// Thrice-nested entropy
        pub entropy: [u8; 96],
    }

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        
        /// The currency mechanism.
        type Currency: ReservableCurrency<Self::AccountId>;
        
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        
        /// The maximum length of a name/symbol/description
        type StringLimit: Get<u32>;
        
        /// The maximum metadata size
        type MetadataLimit: Get<u32>;
        
        /// The maximum number of circuits per profile
        type MaxCircuitsPerProfile: Get<u32>;
        
        /// The maximum number of zones per profile
        type MaxZonesPerProfile: Get<u32>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::storage]
    #[pallet::getter(fn next_token_id)]
    pub type NextTokenId<T> = StorageValue<_, TokenId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn next_profile_id)]
    pub type NextProfileId<T> = StorageValue<_, ProfileId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn next_zone_id)]
    pub type NextZoneId<T> = StorageValue<_, ZoneId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn next_circuit_id)]
    pub type NextCircuitId<T> = StorageValue<_, CircuitId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn tokens)]
    pub type Tokens<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        TokenId,
        RPCQ999Token<T::AccountId, BalanceOf<T>>,
        OptionQuery
    >;

    #[pallet::storage]
    #[pallet::getter(fn profiles)]
    pub type Profiles<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        ProfileId,
        ActorxProfile<T::AccountId, BalanceOf<T>>,
        OptionQuery
    >;

    #[pallet::storage]
    #[pallet::getter(fn zones)]
    pub type Zones<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        ZoneId,
        ActorxZone<T::AccountId, BalanceOf<T>>,
        OptionQuery
    >;

    #[pallet::storage]
    #[pallet::getter(fn circuits)]
    pub type Circuits<T> = StorageMap<
        _,
        Blake2_128Concat,
        CircuitId,
        ActorxCircuit,
        OptionQuery
    >;

    #[pallet::storage]
    #[pallet::getter(fn account_profiles)]
    pub type AccountProfiles<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Vec<ProfileId>,
        ValueQuery
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Token created [creator, token_id, name]
        TokenCreated(T::AccountId, TokenId, Vec<u8>),
        
        /// Token transferred [from, to, token_id]
        TokenTransferred(T::AccountId, T::AccountId, TokenId),
        
        /// Profile created [owner, profile_id, name]
        ProfileCreated(T::AccountId, ProfileId, Vec<u8>),
        
        /// Zone created [controller, zone_id, name]
        ZoneCreated(T::AccountId, ZoneId, Vec<u8>),
        
        /// Circuit created [circuit_id, name]
        CircuitCreated(CircuitId, Vec<u8>),
        
        /// Profile joined zone [profile_id, zone_id]
        ProfileJoinedZone(ProfileId, ZoneId),
        
        /// Circuit added to profile [profile_id, circuit_id]
        CircuitAddedToProfile(ProfileId, CircuitId),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Token does not exist
        TokenNotFound,
        /// Profile does not exist
        ProfileNotFound,
        /// Zone does not exist
        ZoneNotFound,
        /// Circuit does not exist
        CircuitNotFound,
        /// Unauthorized operation
        Unauthorized,
        /// Name too long
        NameTooLong,
        /// Description too long
        DescriptionTooLong,
        /// Metadata too large
        MetadataTooLarge,
        /// Max circuits reached
        MaxCircuitsReached,
        /// Max zones reached
        MaxZonesReached,
        /// Invalid transfer
        InvalidTransfer,
        /// Profile not in required zone
        ProfileNotInZone,
        /// Insufficient profile level
        InsufficientProfileLevel,
        /// Math overflow
        MathOverflow,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Create a new RPC-Q999 token
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::create_token())]
        pub fn create_token(
            origin: OriginFor<T>,
            name: Vec<u8>,
            symbol: Vec<u8>,
            metadata: Vec<u8>,
            circuit_id: Option<CircuitId>,
            required_zones: Vec<ZoneId>,
            min_profile_level: u8,
            initial_balance: BalanceOf<T>,
        ) -> DispatchResult {
            let creator = ensure_signed(origin)?;
            
            // Validate inputs
            ensure!(name.len() <= T::StringLimit::get() as usize, Error::<T>::NameTooLong);
            ensure!(symbol.len() <= T::StringLimit::get() as usize, Error::<T>::NameTooLong);
            ensure!(metadata.len() <= T::MetadataLimit::get() as usize, Error::<T>::MetadataTooLarge);
            
            // Check if circuit exists if specified
            if let Some(cid) = circuit_id {
                ensure!(<Circuits<T>>::contains_key(cid), Error::<T>::CircuitNotFound);
            }
            
            // Check if all required zones exist
            for zone_id in &required_zones {
                ensure!(<Zones<T>>::contains_key(zone_id), Error::<T>::ZoneNotFound);
            }
            
            // Generate quantum entropy
            let entropy = Self::generate_token_entropy(&creator, &name, &symbol);
            
            // Create token
            let token_id = Self::next_token_id();
            let token = RPCQ999Token {
                id: token_id,
                owner: creator.clone(),
                name: name.clone(),
                symbol,
                metadata,
                circuit: circuit_id,
                balance: initial_balance,
                required_zones,
                min_profile_level,
                entropy,
            };
            
            // Store token
            <Tokens<T>>::insert(token_id, token);
            <NextTokenId<T>>::put(token_id.checked_add(1).ok_or(Error::<T>::MathOverflow)?);
            
            // Emit event
            Self::deposit_event(Event::TokenCreated(creator, token_id, name));
            
            Ok(())
        }
        
        /// Create a new Actorx profile
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::create_profile())]
        pub fn create_profile(
            origin: OriginFor<T>,
            name: Vec<u8>,
            metadata: Vec<u8>,
            social_identities: Vec<Vec<u8>>,
            staked_amount: BalanceOf<T>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            
            // Validate inputs
            ensure!(name.len() <= T::StringLimit::get() as usize, Error::<T>::NameTooLong);
            ensure!(metadata.len() <= T::MetadataLimit::get() as usize, Error::<T>::MetadataTooLarge);
            
            // Generate quantum entropy
            let entropy = Self::generate_profile_entropy(&owner, &name);
            
            // Create profile
            let profile_id = Self::next_profile_id();
            let profile = ActorxProfile {
                id: profile_id,
                owner: owner.clone(),
                name: name.clone(),
                circuits: Vec::new(),
                zones: Vec::new(),
                metadata,
                social_identities,
                staked: staked_amount,
                entropy,
            };
            
            // Reserve the staked amount
            if !staked_amount.is_zero() {
                T::Currency::reserve(&owner, staked_amount)?;
            }
            
            // Store profile
            <Profiles<T>>::insert(profile_id, profile);
            <NextProfileId<T>>::put(profile_id.checked_add(1).ok_or(Error::<T>::MathOverflow)?);
            
            // Add profile to account's profiles
            let mut account_profiles = <AccountProfiles<T>>::get(&owner);
            account_profiles.push(profile_id);
            <AccountProfiles<T>>::insert(&owner, account_profiles);
            
            // Emit event
            Self::deposit_event(Event::ProfileCreated(owner, profile_id, name));
            
            Ok(())
        }
        
        // Additional extrinsics would be implemented here
    }
    
    // Internal helper functions
    impl<T: Config> Pallet<T> {
        /// Generate quantum entropy for a token using thrice-nested approach
        fn generate_token_entropy(
            account: &T::AccountId,
            name: &[u8],
            symbol: &[u8],
        ) -> [u8; 96] {
            let account_bytes = account.encode();
            
            // Generate Fe layer (outer)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&account_bytes);
            hasher.update(name);
            let fe_layer = hasher.finalize().into();
            
            // Generate Ni layer (middle)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(symbol);
            let ni_layer = hasher.finalize().into();
            
            // Generate Co layer (inner)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&ni_layer);
            hasher.update(&account_bytes);
            let co_layer = hasher.finalize().into();
            
            // Combine all layers
            let mut result = [0u8; 96];
            result[0..32].copy_from_slice(&fe_layer);
            result[32..64].copy_from_slice(&ni_layer);
            result[64..96].copy_from_slice(&co_layer);
            
            result
        }
        
        /// Generate quantum entropy for a profile
        fn generate_profile_entropy(
            account: &T::AccountId,
            name: &[u8],
        ) -> [u8; 96] {
            let account_bytes = account.encode();
            let timestamp = <frame_system::Pallet<T>>::block_number().encode();
            
            // Generate Fe layer (outer)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&account_bytes);
            hasher.update(name);
            let fe_layer = hasher.finalize().into();
            
            // Generate Ni layer (middle)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&timestamp);
            let ni_layer = hasher.finalize().into();
            
            // Generate Co layer (inner)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&ni_layer);
            hasher.update(&account_bytes);
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
        fn create_token() -> Weight;
        fn create_profile() -> Weight;
        fn create_zone() -> Weight;
        fn create_circuit() -> Weight;
        fn transfer_token() -> Weight;
        fn join_zone() -> Weight;
        fn add_circuit_to_profile() -> Weight;
    }
}
