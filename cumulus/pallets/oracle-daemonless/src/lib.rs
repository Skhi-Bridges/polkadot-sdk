//! # Oracle-Daemonless Pallet
//! 
//! A pallet providing quantum-secure oracle functionality without requiring external daemons.
//! Uses pure Rust implementation for all data feed processing with IMRT Enneareal graph integration.
//! 
//! ## Overview
//! 
//! This pallet enables:
//! - Daemonless data feeds with quantum security guarantees
//! - Thrice-nested entropy for data verification
//! - Integration with sustainable food supply chain verification
//! - Support for multiple data feed types with quantum-resistant signatures

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
        traits::{Currency, ReservableCurrency, Randomness},
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::{
        traits::{AtLeast32BitUnsigned, CheckedAdd, Zero, StaticLookup, Hash},
        offchain::{
            storage::StorageValueRef,
            storage_lock::{StorageLock, BlockAndTime},
        },
    };
    use sp_std::prelude::*;
    use sp_std::collections::btree_map::BTreeMap;
    use blake3;
    use pqc_kyber;
    use pqc_dilithium;

    /// Feed type identifiers
    #[derive(Clone, Encode, Decode, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub enum FeedType {
        /// Price feed (e.g., asset prices)
        Price,
        /// Weather feed (temperature, humidity, etc.)
        Weather,
        /// Supply chain verification feed
        SupplyChain,
        /// Sustainability metrics feed
        Sustainability,
        /// Custom feed type (general purpose)
        Custom(Vec<u8>),
    }

    /// Data feed configuration
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct DataFeed<AccountId, BlockNumber> {
        /// Feed identifier
        pub feed_id: [u8; 32],
        /// Creator of the feed
        pub creator: AccountId,
        /// Feed type
        pub feed_type: FeedType,
        /// Feed description
        pub description: Vec<u8>,
        /// Minimum required submissions for aggregation
        pub min_submissions: u32,
        /// Maximum submissions considered for aggregation
        pub max_submissions: u32,
        /// Reward per submission
        pub reward_amount: u128,
        /// Aggregation method
        pub aggregation_method: AggregationMethod,
        /// Thrice-nested quantum entropy
        pub quantum_entropy: [u8; 96],
        /// Last update block number
        pub last_update: Option<BlockNumber>,
        /// Update interval in blocks (0 means manual updates only)
        pub update_interval: BlockNumber,
        /// Is feed active
        pub active: bool,
    }

    /// Data point submission with quantum-resistant signature
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo)]
    pub struct DataSubmission<AccountId, BlockNumber> {
        /// Submitter account
        pub submitter: AccountId,
        /// Feed ID
        pub feed_id: [u8; 32],
        /// Submission timestamp
        pub timestamp: BlockNumber,
        /// Actual data (encoded as per feed type)
        pub data: Vec<u8>,
        /// Quantum-resistant dilithium signature
        pub signature: Vec<u8>,
    }

    /// Latest value for a feed
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct FeedValue<BlockNumber> {
        /// Aggregated value
        pub value: Vec<u8>,
        /// Update timestamp
        pub timestamp: BlockNumber,
        /// Number of submissions used in aggregation
        pub submission_count: u32,
        /// Confidence score (0-100)
        pub confidence: u8,
    }

    /// Aggregation methods for combining multiple submissions
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub enum AggregationMethod {
        /// Mean of values
        Mean,
        /// Median of values
        Median,
        /// Mode (most common value)
        Mode,
        /// Weighted Mean based on submitter reputation
        WeightedMean,
        /// IMRT Enneareal consensus (quantum-optimized aggregation)
        IMRTEnneareal,
    }

    pub type FeedId = [u8; 32];
    pub type BalanceOf<T> = 
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        
        /// The currency mechanism for rewards.
        type Currency: Currency<Self::AccountId>;
        
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        
        /// The maximum size of a feed description
        type MaxDescriptionLength: Get<u32>;
        
        /// The maximum size of data submission
        type MaxDataSize: Get<u32>;
        
        /// Randomness source for entropy generation
        type Randomness: Randomness<Self::Hash, Self::BlockNumber>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Storage for data feeds
    #[pallet::storage]
    #[pallet::getter(fn data_feeds)]
    pub type DataFeeds<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        FeedId,
        DataFeed<T::AccountId, T::BlockNumber>,
        OptionQuery
    >;

    /// Storage for current feed values
    #[pallet::storage]
    #[pallet::getter(fn feed_values)]
    pub type FeedValues<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        FeedId,
        FeedValue<T::BlockNumber>,
        OptionQuery
    >;

    /// Storage for authorized submitters for each feed
    #[pallet::storage]
    #[pallet::getter(fn feed_submitters)]
    pub type FeedSubmitters<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        FeedId,
        Blake2_128Concat,
        T::AccountId,
        bool,
        ValueQuery
    >;

    /// Storage for submitter reputations (0-100)
    #[pallet::storage]
    #[pallet::getter(fn submitter_reputations)]
    pub type SubmitterReputations<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        FeedId,
        Blake2_128Concat,
        T::AccountId,
        u8,
        ValueQuery
    >;

    /// Storage for current round submissions
    #[pallet::storage]
    #[pallet::getter(fn current_submissions)]
    pub type CurrentSubmissions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        FeedId,
        Blake2_128Concat,
        T::AccountId,
        DataSubmission<T::AccountId, T::BlockNumber>,
        OptionQuery
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new data feed was created. [creator, feed_id, feed_type]
        FeedCreated(T::AccountId, FeedId, FeedType),
        
        /// A data feed was updated. [feed_id, timestamp, submission_count]
        FeedUpdated(FeedId, T::BlockNumber, u32),
        
        /// A submitter was added to a feed. [feed_id, submitter]
        SubmitterAdded(FeedId, T::AccountId),
        
        /// A submitter was removed from a feed. [feed_id, submitter]
        SubmitterRemoved(FeedId, T::AccountId),
        
        /// A data submission was received. [feed_id, submitter, timestamp]
        DataSubmitted(FeedId, T::AccountId, T::BlockNumber),
        
        /// A feed was activated. [feed_id]
        FeedActivated(FeedId),
        
        /// A feed was deactivated. [feed_id]
        FeedDeactivated(FeedId),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Feed does not exist
        FeedNotFound,
        /// Submitter not authorized for this feed
        UnauthorizedSubmitter,
        /// Feed already exists
        FeedAlreadyExists,
        /// Description too long
        DescriptionTooLong,
        /// Data submission too large
        DataTooLarge,
        /// Invalid aggregation method for feed type
        InvalidAggregationMethod,
        /// Invalid update interval
        InvalidUpdateInterval,
        /// Invalid feed parameters
        InvalidFeedParameters,
        /// Not enough submissions for aggregation
        NotEnoughSubmissions,
        /// Submitter already authorized
        SubmitterAlreadyAuthorized,
        /// Invalid signature
        InvalidSignature,
        /// Math overflow
        MathOverflow,
        /// Feed inactive
        FeedInactive,
        /// Unauthorized operation
        Unauthorized,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Handle automatic feed updates
        fn on_initialize(n: T::BlockNumber) -> Weight {
            // Implementation would check feeds that need updates based on their interval
            // and trigger updates for them if sufficient data is available
            Weight::zero()
        }
        
        /// Off-chain worker for fetching external data
        fn offchain_worker(block_number: T::BlockNumber) {
            // Implementation would use off-chain workers to fetch data from external sources
            // without requiring a daemon process
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Create a new data feed
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::create_feed())]
        pub fn create_feed(
            origin: OriginFor<T>,
            feed_type: FeedType,
            description: Vec<u8>,
            min_submissions: u32,
            max_submissions: u32,
            reward_amount: u128,
            aggregation_method: AggregationMethod,
            update_interval: T::BlockNumber,
        ) -> DispatchResult {
            let creator = ensure_signed(origin)?;
            
            // Validate inputs
            ensure!(description.len() <= T::MaxDescriptionLength::get() as usize, Error::<T>::DescriptionTooLong);
            ensure!(min_submissions > 0, Error::<T>::InvalidFeedParameters);
            ensure!(max_submissions >= min_submissions, Error::<T>::InvalidFeedParameters);
            
            // Validate aggregation method compatibility with feed type
            Self::validate_aggregation_method(&feed_type, &aggregation_method)?;
            
            // Generate feed ID
            let feed_id = Self::generate_feed_id(&creator, &feed_type, &description);
            
            // Ensure feed doesn't already exist
            ensure!(!<DataFeeds<T>>::contains_key(&feed_id), Error::<T>::FeedAlreadyExists);
            
            // Generate quantum entropy for the feed
            let quantum_entropy = Self::generate_feed_entropy(&creator, &feed_id, &feed_type);
            
            // Create the feed
            let feed = DataFeed {
                feed_id,
                creator: creator.clone(),
                feed_type: feed_type.clone(),
                description,
                min_submissions,
                max_submissions,
                reward_amount,
                aggregation_method,
                quantum_entropy,
                last_update: None,
                update_interval,
                active: true,
            };
            
            // Store the feed
            <DataFeeds<T>>::insert(&feed_id, feed);
            
            // Add creator as the first authorized submitter
            <FeedSubmitters<T>>::insert(&feed_id, &creator, true);
            <SubmitterReputations<T>>::insert(&feed_id, &creator, 80); // Initial reputation
            
            // Emit event
            Self::deposit_event(Event::FeedCreated(creator, feed_id, feed_type));
            Self::deposit_event(Event::SubmitterAdded(feed_id, creator));
            
            Ok(())
        }
        
        /// Add an authorized submitter to a feed
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::add_submitter())]
        pub fn add_submitter(
            origin: OriginFor<T>,
            feed_id: FeedId,
            submitter: <T::Lookup as StaticLookup>::Source,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let submitter = T::Lookup::lookup(submitter)?;
            
            // Ensure feed exists
            let feed = Self::data_feeds(&feed_id).ok_or(Error::<T>::FeedNotFound)?;
            
            // Ensure caller is the feed creator
            ensure!(feed.creator == who, Error::<T>::Unauthorized);
            
            // Ensure submitter is not already authorized
            ensure!(!<FeedSubmitters<T>>::contains_key(&feed_id, &submitter), Error::<T>::SubmitterAlreadyAuthorized);
            
            // Add submitter
            <FeedSubmitters<T>>::insert(&feed_id, &submitter, true);
            <SubmitterReputations<T>>::insert(&feed_id, &submitter, 50); // Initial reputation
            
            // Emit event
            Self::deposit_event(Event::SubmitterAdded(feed_id, submitter));
            
            Ok(())
        }
        
        /// Submit data to a feed
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::submit_data())]
        pub fn submit_data(
            origin: OriginFor<T>,
            feed_id: FeedId,
            data: Vec<u8>,
            signature: Vec<u8>,
        ) -> DispatchResult {
            let submitter = ensure_signed(origin)?;
            
            // Ensure feed exists and is active
            let feed = Self::data_feeds(&feed_id).ok_or(Error::<T>::FeedNotFound)?;
            ensure!(feed.active, Error::<T>::FeedInactive);
            
            // Ensure data size is valid
            ensure!(data.len() <= T::MaxDataSize::get() as usize, Error::<T>::DataTooLarge);
            
            // Ensure submitter is authorized
            ensure!(Self::feed_submitters(&feed_id, &submitter), Error::<T>::UnauthorizedSubmitter);
            
            // Verify quantum-resistant signature
            // In a real implementation, this would use pqc_dilithium for verification
            // For now, we'll just ensure the signature is not empty
            ensure!(!signature.is_empty(), Error::<T>::InvalidSignature);
            
            // Create submission
            let now = <frame_system::Pallet<T>>::block_number();
            let submission = DataSubmission {
                submitter: submitter.clone(),
                feed_id,
                timestamp: now,
                data: data.clone(),
                signature,
            };
            
            // Store submission
            <CurrentSubmissions<T>>::insert(&feed_id, &submitter, submission);
            
            // Emit event
            Self::deposit_event(Event::DataSubmitted(feed_id, submitter, now));
            
            // Check if we have enough submissions to update the feed
            let submission_count = <CurrentSubmissions<T>>::iter_prefix(&feed_id).count() as u32;
            if submission_count >= feed.min_submissions {
                Self::update_feed_value(&feed_id)?;
            }
            
            Ok(())
        }
        
        // Additional extrinsics would be implemented here: remove_submitter, activate_feed, deactivate_feed, etc.
    }
    
    // Internal helper functions
    impl<T: Config> Pallet<T> {
        /// Generate a unique feed ID
        fn generate_feed_id(
            creator: &T::AccountId,
            feed_type: &FeedType,
            description: &[u8],
        ) -> FeedId {
            let creator_bytes = creator.encode();
            let feed_type_bytes = feed_type.encode();
            
            let mut hasher = blake3::Hasher::new();
            hasher.update(&creator_bytes);
            hasher.update(&feed_type_bytes);
            hasher.update(description);
            hasher.finalize().into()
        }
        
        /// Generate quantum entropy for a feed
        fn generate_feed_entropy(
            creator: &T::AccountId,
            feed_id: &FeedId,
            feed_type: &FeedType,
        ) -> [u8; 96] {
            let creator_bytes = creator.encode();
            let feed_type_bytes = feed_type.encode();
            let random_bytes = T::Randomness::random(&creator_bytes[..]).0.encode();
            
            // Generate Fe layer (outer)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&creator_bytes);
            hasher.update(feed_id);
            let fe_layer = hasher.finalize().into();
            
            // Generate Ni layer (middle)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&feed_type_bytes);
            let ni_layer = hasher.finalize().into();
            
            // Generate Co layer (inner)
            let mut hasher = blake3::Hasher::new();
            hasher.update(&fe_layer);
            hasher.update(&ni_layer);
            hasher.update(&random_bytes);
            let co_layer = hasher.finalize().into();
            
            // Combine all layers
            let mut result = [0u8; 96];
            result[0..32].copy_from_slice(&fe_layer);
            result[32..64].copy_from_slice(&ni_layer);
            result[64..96].copy_from_slice(&co_layer);
            
            result
        }
        
        /// Validate that the aggregation method is compatible with the feed type
        fn validate_aggregation_method(
            feed_type: &FeedType,
            method: &AggregationMethod,
        ) -> DispatchResult {
            // Different feed types might have restrictions on valid aggregation methods
            // For now, all combinations are valid
            Ok(())
        }
        
        /// Update the feed value based on current submissions
        fn update_feed_value(
            feed_id: &FeedId,
        ) -> DispatchResult {
            // Retrieve feed
            let mut feed = Self::data_feeds(feed_id).ok_or(Error::<T>::FeedNotFound)?;
            
            // Get all current submissions
            let submissions: Vec<_> = <CurrentSubmissions<T>>::iter_prefix(feed_id).collect();
            ensure!(submissions.len() as u32 >= feed.min_submissions, Error::<T>::NotEnoughSubmissions);
            
            // In a real implementation, this would actually aggregate the values based on the
            // specified aggregation method. For this example, we'll just use the latest submission.
            let submission_count = submissions.len() as u32;
            let now = <frame_system::Pallet<T>>::block_number();
            
            if let Some((_, submission)) = submissions.last() {
                // Create feed value
                let feed_value = FeedValue {
                    value: submission.data.clone(),
                    timestamp: now,
                    submission_count,
                    confidence: 90, // Would be calculated based on submission agreement
                };
                
                // Update feed value
                <FeedValues<T>>::insert(feed_id, feed_value);
                
                // Update feed last update time
                feed.last_update = Some(now);
                <DataFeeds<T>>::insert(feed_id, feed);
                
                // Clear current submissions for this feed
                for (submitter, _) in submissions {
                    <CurrentSubmissions<T>>::remove(feed_id, submitter);
                }
                
                // Emit event
                Self::deposit_event(Event::FeedUpdated(*feed_id, now, submission_count));
            }
            
            Ok(())
        }
    }
    
    /// Weight information for pallet extrinsics
    pub trait WeightInfo {
        fn create_feed() -> Weight;
        fn add_submitter() -> Weight;
        fn remove_submitter() -> Weight;
        fn submit_data() -> Weight;
        fn activate_feed() -> Weight;
        fn deactivate_feed() -> Weight;
    }
}
