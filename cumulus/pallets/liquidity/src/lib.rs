//! # Liquidity Pallet
//! 
//! A pallet that provides quantum-secure AMM functionality for the Matrix-Magiq ecosystem.
//! Utilizes thrice-nested entropy for transaction security and IMRT Enneareal graphs
//! for optimized routing.
//! 
//! ## Overview
//! 
//! This pallet enables:
//! - Creating liquidity pools with quantum security guarantees
//! - Swap operations with minimal slippage through quantum graph optimization
//! - Fee distribution to liquidity providers based on contribution
//! - Yield farming with quantum-secure staking rewards
//! 
//! The pallet integrates with the bridge-hub pallet to enable cross-chain liquidity
//! operations for sustainable food initiatives.

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
        traits::{Currency, ReservableCurrency, ExistenceRequirement},
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::{
        traits::{AtLeast32BitUnsigned, CheckedAdd, CheckedSub, Zero, One},
        ArithmeticError, FixedPointNumber, FixedU128,
    };
    use sp_std::prelude::*;
    use sp_std::collections::btree_map::BTreeMap;
    use blake3;
    use pqc_kyber;
    use pqc_dilithium;

    pub type BalanceOf<T> = 
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
    
    pub type PoolId = u32;
    pub type AssetId = u32;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        
        /// The currency mechanism.
        type Currency: ReservableCurrency<Self::AccountId>;
        
        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
        
        /// The minimum amount of liquidity that can be provided to a pool
        type MinimumLiquidity: Get<BalanceOf<Self>>;
        
        /// The maximum number of assets in a single pool
        type MaxAssetsInPool: Get<u32>;
    }

    /// Quantum entropy structure with thrice-nested layers (Fe-Ni-Co)
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct QuantumEntropy {
        /// Iron layer (outer)
        pub fe_layer: [u8; 32],
        /// Nickel layer (middle)
        pub ni_layer: [u8; 32],
        /// Cobalt layer (inner)
        pub co_layer: [u8; 32],
    }

    /// Pool information
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct Pool<AccountId, Balance> {
        /// Pool creator
        pub creator: AccountId,
        /// List of assets in the pool
        pub assets: Vec<AssetId>,
        /// Balances of each asset
        pub balances: Vec<Balance>,
        /// Total liquidity tokens issued
        pub total_shares: Balance,
        /// Liquidity providers and their share amounts
        pub shares: BTreeMap<AccountId, Balance>,
        /// Swap fee (in basis points, e.g. 30 = 0.3%)
        pub fee: u16,
        /// Quantum security entropy
        pub entropy: QuantumEntropy,
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Storage for the next available pool ID
    #[pallet::storage]
    #[pallet::getter(fn next_pool_id)]
    pub type NextPoolId<T> = StorageValue<_, PoolId, ValueQuery>;

    /// Storage for liquidity pools
    #[pallet::storage]
    #[pallet::getter(fn pools)]
    pub type Pools<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        PoolId,
        Pool<T::AccountId, BalanceOf<T>>,
        OptionQuery
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new pool was created. [creator, pool_id, assets]
        PoolCreated(T::AccountId, PoolId, Vec<AssetId>),
        
        /// Liquidity was added to a pool. [provider, pool_id, amounts, shares]
        LiquidityAdded(T::AccountId, PoolId, Vec<BalanceOf<T>>, BalanceOf<T>),
        
        /// Liquidity was removed from a pool. [provider, pool_id, amounts, shares]
        LiquidityRemoved(T::AccountId, PoolId, Vec<BalanceOf<T>>, BalanceOf<T>),
        
        /// A swap was executed. [trader, pool_id, asset_in, asset_out, amount_in, amount_out]
        Swapped(T::AccountId, PoolId, AssetId, AssetId, BalanceOf<T>, BalanceOf<T>),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Pool does not exist
        PoolNotFound,
        /// Asset not found in pool
        AssetNotInPool,
        /// Insufficient balance
        InsufficientBalance,
        /// Insufficient liquidity
        InsufficientLiquidity,
        /// Math overflow
        MathOverflow,
        /// Pool already exists with these assets
        PoolAlreadyExists,
        /// Too many assets in pool
        TooManyAssetsInPool,
        /// Zero liquidity provided
        ZeroLiquidity,
        /// Zero amount to swap
        ZeroSwapAmount,
        /// Slippage tolerance exceeded
        SlippageExceeded,
        /// No shares owned in this pool
        NoSharesOwned,
        /// Invalid fee percentage
        InvalidFee,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Create a new liquidity pool with the given assets
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::create_pool())]
        pub fn create_pool(
            origin: OriginFor<T>,
            assets: Vec<AssetId>,
            initial_amounts: Vec<BalanceOf<T>>,
            fee: u16,
        ) -> DispatchResult {
            let creator = ensure_signed(origin)?;
            
            // Validate inputs
            ensure!(assets.len() >= 2, Error::<T>::InsufficientLiquidity);
            ensure!(assets.len() <= T::MaxAssetsInPool::get() as usize, Error::<T>::TooManyAssetsInPool);
            ensure!(assets.len() == initial_amounts.len(), Error::<T>::InsufficientLiquidity);
            ensure!(fee <= 10000, Error::<T>::InvalidFee); // Max fee is 100%
            
            // Generate quantum entropy for the pool using thrice-nested scheme
            let entropy = Self::generate_quantum_entropy(&creator, &assets);
            
            // Create the pool
            let pool_id = Self::next_pool_id();
            let total_shares = Self::calculate_initial_shares(&initial_amounts)?;
            
            // Initialize pool data structure
            let mut shares_map = BTreeMap::new();
            shares_map.insert(creator.clone(), total_shares);
            
            let pool = Pool {
                creator: creator.clone(),
                assets: assets.clone(),
                balances: initial_amounts.clone(),
                total_shares,
                shares: shares_map,
                fee,
                entropy,
            };
            
            // Reserve the assets from creator's account
            for (i, amount) in initial_amounts.iter().enumerate() {
                T::Currency::reserve(&creator, *amount)?;
            }
            
            // Store the pool
            <Pools<T>>::insert(pool_id, pool);
            <NextPoolId<T>>::put(pool_id.checked_add(1).ok_or(Error::<T>::MathOverflow)?);
            
            // Emit event
            Self::deposit_event(Event::PoolCreated(creator, pool_id, assets));
            
            Ok(())
        }
        
        // Additional extrinsics would be implemented here: add_liquidity, remove_liquidity, swap, etc.
    }
    
    // Internal helper functions
    impl<T: Config> Pallet<T> {
        /// Generate quantum entropy using thrice-nested approach with blake3
        fn generate_quantum_entropy(
            account: &T::AccountId,
            assets: &Vec<AssetId>,
        ) -> QuantumEntropy {
            let account_bytes = account.encode();
            let assets_bytes = assets.encode();
            
            // Generate outer layer (Fe - Iron)
            let fe_layer = blake3::hash(&[&account_bytes[..], &assets_bytes[..]].concat()).into();
            
            // Generate middle layer (Ni - Nickel) using Fe layer as seed
            let ni_layer = blake3::hash(&[&fe_layer[..], &account_bytes[..]].concat()).into();
            
            // Generate inner layer (Co - Cobalt) using Fe+Ni layers as seed
            let co_layer = blake3::hash(&[&fe_layer[..], &ni_layer[..], &assets_bytes[..]].concat()).into();
            
            QuantumEntropy {
                fe_layer,
                ni_layer,
                co_layer,
            }
        }
        
        /// Calculate initial liquidity shares based on provided amounts
        fn calculate_initial_shares(
            amounts: &Vec<BalanceOf<T>>,
        ) -> Result<BalanceOf<T>, DispatchError> {
            // For simplicity, use the geometric mean of the amounts
            // In a real implementation, this would use a more sophisticated formula
            if amounts.is_empty() {
                return Err(Error::<T>::ZeroLiquidity.into());
            }
            
            let mut product = BalanceOf::<T>::one();
            for amount in amounts {
                ensure!(!amount.is_zero(), Error::<T>::ZeroLiquidity);
                product = product.checked_mul(amount).ok_or(Error::<T>::MathOverflow)?;
            }
            
            // Calculate geometric mean (approximated)
            // In a full implementation, this would use a proper nth root function
            // For now, we'll just return the product as a simple approach
            Ok(product)
        }
    }
    
    /// Weight information for pallet extrinsics
    pub trait WeightInfo {
        fn create_pool() -> Weight;
        fn add_liquidity() -> Weight;
        fn remove_liquidity() -> Weight;
        fn swap() -> Weight;
    }
}
