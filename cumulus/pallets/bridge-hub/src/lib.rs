//! # Bridge Hub Pallet
//!
//! A central hub for connecting Matrix-Magiq to multiple blockchains for community outreach
//! and sustainable food culture initiatives.
//!
//! The pallet provides:
//! - Bridge channel management with quantum security
//! - Cross-chain message routing with thrice-nested entropy
//! - Community proposal forwarding for sustainable food initiatives
//! - Supply chain verification across multiple blockchains

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub mod weights;

#[frame_support::pallet]
pub mod pallet {
    use codec::{Decode, Encode, MaxEncodedLen};
    use cumulus_bridge_core::{
        BridgeAccountId, BridgeConfig, BridgeFeeModel, BridgeMessage, BridgeMessageType,
        BridgeResult, BlockchainPlatform, SecurityConfig, EntropyProtocol,
        community::{CommunityProposal, SupplyChainEntry, SupplyChainStage},
        utils,
    };
    use cumulus_primitives_quantum_channel::{
        QuantumKeyPair, QuantumProtocol, ThreeNestedSecurityLevel,
    };
    use frame_support::{
        dispatch::{DispatchResult, DispatchResultWithPostInfo},
        pallet_prelude::*,
        traits::{Currency, ExistenceRequirement, ReservableCurrency, WithdrawReasons},
        transactional,
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::{CheckedAdd, CheckedSub, SaturatedConversion, StaticLookup};
    use sp_std::prelude::*;

    pub type BalanceOf<T> =
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The currency mechanism, used for paying transaction fees.
        type Currency: ReservableCurrency<Self::AccountId>;

        /// Origin that can configure bridges (usually root).
        type BridgeAdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// Maximum supported bridges.
        #[pallet::constant]
        type MaxBridges: Get<u32>;

        /// Maximum message size.
        #[pallet::constant]
        type MaxMessageSize: Get<u32>;

        /// Maximum keys per bridge channel.
        #[pallet::constant]
        type MaxKeysPerChannel: Get<u32>;

        /// The weight information for this pallet.
        type WeightInfo: weights::WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Bridge configurations by platform.
    #[pallet::storage]
    pub(super) type Bridges<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        BlockchainPlatform,
        BridgeConfig,
        OptionQuery,
    >;

    /// Quantum keys for bridge channels.
    #[pallet::storage]
    pub(super) type BridgeKeys<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        BlockchainPlatform,
        Blake2_128Concat,
        u32, // Key index
        QuantumKeyPair,
        OptionQuery,
    >;

    /// Next key index for a bridge.
    #[pallet::storage]
    pub(super) type NextKeyIndex<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        BlockchainPlatform,
        u32,
        ValueQuery,
    >;

    /// Outbound messages waiting to be confirmed.
    #[pallet::storage]
    pub(super) type OutboundMessages<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        BlockchainPlatform,
        Blake2_128Concat,
        T::Hash, // Message hash
        (BridgeMessage, T::AccountId), // Message and sender
        OptionQuery,
    >;

    /// Inbound messages that have been verified and are waiting to be processed.
    #[pallet::storage]
    pub(super) type InboundMessages<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        BlockchainPlatform,
        Blake2_128Concat,
        T::Hash, // Message hash
        BridgeMessage,
        OptionQuery,
    >;

    /// Community proposals by source chain and ID.
    #[pallet::storage]
    pub(super) type CommunityProposals<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        BlockchainPlatform,
        Blake2_128Concat,
        Vec<u8>, // Proposal ID
        CommunityProposal,
        OptionQuery,
    >;

    /// Supply chain entries by product ID.
    #[pallet::storage]
    pub(super) type SupplyChainEntries<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        Vec<u8>, // Product ID
        Vec<SupplyChainEntry>, // Historical entries
        ValueQuery,
    >;

    /// Fee reserves for bridge operations.
    #[pallet::storage]
    pub(super) type BridgeFeeReserves<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        BlockchainPlatform,
        BalanceOf<T>,
        ValueQuery,
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A new bridge has been registered.
        BridgeRegistered {
            platform: BlockchainPlatform,
            config: BridgeConfig,
        },
        /// A bridge configuration has been updated.
        BridgeUpdated {
            platform: BlockchainPlatform,
            config: BridgeConfig,
        },
        /// A bridge has been deactivated.
        BridgeDeactivated {
            platform: BlockchainPlatform,
        },
        /// A new quantum key has been generated for a bridge.
        QuantumKeyGenerated {
            platform: BlockchainPlatform,
            key_index: u32,
            protocol: QuantumProtocol,
        },
        /// An outbound message has been sent.
        MessageSent {
            platform: BlockchainPlatform,
            message_hash: T::Hash,
            message_type: BridgeMessageType,
            sender: T::AccountId,
        },
        /// An outbound message has been confirmed.
        MessageConfirmed {
            platform: BlockchainPlatform,
            message_hash: T::Hash,
            transaction_hash: Vec<u8>,
        },
        /// An inbound message has been received.
        MessageReceived {
            platform: BlockchainPlatform,
            message_hash: T::Hash,
            message_type: BridgeMessageType,
        },
        /// A community proposal has been registered.
        CommunityProposalRegistered {
            platform: BlockchainPlatform,
            proposal_id: Vec<u8>,
            title: Vec<u8>,
            funding_amount: u128,
        },
        /// A supply chain entry has been verified.
        SupplyChainEntryVerified {
            product_id: Vec<u8>,
            stage: SupplyChainStage,
            verification_hash: [u8; 32],
        },
        /// Fees have been collected for a bridge operation.
        FeesCollected {
            platform: BlockchainPlatform,
            amount: BalanceOf<T>,
            account: T::AccountId,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Bridge already exists.
        BridgeAlreadyExists,
        /// Bridge does not exist.
        BridgeDoesNotExist,
        /// Bridge is not active.
        BridgeNotActive,
        /// Message is too large.
        MessageTooLarge,
        /// Invalid message format.
        InvalidMessage,
        /// Invalid signature.
        InvalidSignature,
        /// No quantum keys available.
        NoQuantumKeysAvailable,
        /// Too many keys for this bridge.
        TooManyKeys,
        /// Message does not exist.
        MessageDoesNotExist,
        /// Fee calculation error.
        FeeCalculationError,
        /// Insufficient balance for fees.
        InsufficientBalance,
        /// Community proposal already exists.
        ProposalAlreadyExists,
        /// Maximum bridges reached.
        MaxBridgesReached,
        /// Supply chain stage regression.
        SupplyChainStageRegression,
        /// Verification hash mismatch.
        VerificationHashMismatch,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_finalize(n: BlockNumberFor<T>) {
            // Rotate keys if needed based on block number
            if n.is_zero() {
                return;
            }
            
            // Every 10000 blocks, generate new quantum keys for active bridges
            if n % 10000u32.saturated_into() == 0u32.saturated_into() {
                for (platform, config) in Bridges::<T>::iter() {
                    if config.active {
                        let _ = Self::generate_quantum_key_internal(
                            platform,
                            QuantumProtocol::ThriceNestedStereo,
                            ThreeNestedSecurityLevel::High,
                        );
                    }
                }
            }
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Register a new blockchain bridge.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::register_bridge())]
        pub fn register_bridge(
            origin: OriginFor<T>,
            platform: BlockchainPlatform,
            endpoint: Vec<u8>,
            contract_address: Option<Vec<u8>>,
            max_message_size: u32,
            required_confirmations: u32,
            use_quantum_security: bool,
            entropy_protocol: EntropyProtocol,
        ) -> DispatchResult {
            T::BridgeAdminOrigin::ensure_origin(origin)?;
            
            ensure!(
                Bridges::<T>::len() < T::MaxBridges::get() as usize,
                Error::<T>::MaxBridgesReached
            );
            
            ensure!(
                !Bridges::<T>::contains_key(&platform),
                Error::<T>::BridgeAlreadyExists
            );
            
            // Create bridge configuration
            let fee_model = BridgeFeeModel::Fixed(1_000_000);
            let security_config = SecurityConfig {
                use_quantum_security,
                required_validators: 3,
                entropy_protocol,
                challenge_timeout: 100,
            };
            
            let config = BridgeConfig {
                platform,
                endpoint: Some(endpoint),
                contract_address: contract_address.clone(),
                max_message_size: max_message_size.min(T::MaxMessageSize::get()),
                required_confirmations,
                active: true,
                fee_model,
                security_config,
            };
            
            // Store bridge configuration
            Bridges::<T>::insert(&platform, config.clone());
            
            // Generate initial quantum keys
            Self::generate_quantum_key_internal(
                platform,
                QuantumProtocol::ThriceNestedStereo,
                ThreeNestedSecurityLevel::High,
            )?;
            
            Self::deposit_event(Event::BridgeRegistered {
                platform,
                config,
            });
            
            Ok(())
        }

        /// Update an existing blockchain bridge.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::update_bridge())]
        pub fn update_bridge(
            origin: OriginFor<T>,
            platform: BlockchainPlatform,
            endpoint: Option<Vec<u8>>,
            contract_address: Option<Vec<u8>>,
            max_message_size: Option<u32>,
            required_confirmations: Option<u32>,
            active: Option<bool>,
        ) -> DispatchResult {
            T::BridgeAdminOrigin::ensure_origin(origin)?;
            
            let mut config = Bridges::<T>::get(&platform).ok_or(Error::<T>::BridgeDoesNotExist)?;
            
            // Update values if provided
            if let Some(endpoint) = endpoint {
                config.endpoint = Some(endpoint);
            }
            
            if let Some(contract_address) = contract_address {
                config.contract_address = Some(contract_address);
            }
            
            if let Some(max_message_size) = max_message_size {
                config.max_message_size = max_message_size.min(T::MaxMessageSize::get());
            }
            
            if let Some(required_confirmations) = required_confirmations {
                config.required_confirmations = required_confirmations;
            }
            
            if let Some(active) = active {
                config.active = active;
            }
            
            // Update bridge configuration
            Bridges::<T>::insert(&platform, config.clone());
            
            Self::deposit_event(Event::BridgeUpdated {
                platform,
                config,
            });
            
            Ok(())
        }

        /// Generate a new quantum key for a bridge.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::generate_quantum_key())]
        pub fn generate_quantum_key(
            origin: OriginFor<T>,
            platform: BlockchainPlatform,
            protocol: QuantumProtocol,
            security_level: ThreeNestedSecurityLevel,
        ) -> DispatchResult {
            T::BridgeAdminOrigin::ensure_origin(origin)?;
            
            ensure!(
                Bridges::<T>::contains_key(&platform),
                Error::<T>::BridgeDoesNotExist
            );
            
            Self::generate_quantum_key_internal(platform, protocol, security_level)?;
            
            Ok(())
        }

        /// Send a message to another blockchain.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::send_message(message.payload.len() as u32))]
        #[transactional]
        pub fn send_message(
            origin: OriginFor<T>,
            platform: BlockchainPlatform,
            target_account: BridgeAccountId,
            message_type: BridgeMessageType,
            payload: Vec<u8>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            
            let config = Bridges::<T>::get(&platform).ok_or(Error::<T>::BridgeDoesNotExist)?;
            ensure!(config.active, Error::<T>::BridgeNotActive);
            ensure!(payload.len() <= config.max_message_size as usize, Error::<T>::MessageTooLarge);
            
            // Calculate and collect fees
            let fee = Self::calculate_fee(&config, &message_type, payload.len() as u32)?;
            let fee_balance = fee.saturated_into();
            
            T::Currency::transfer(
                &sender,
                &Self::account_id(),
                fee_balance,
                ExistenceRequirement::KeepAlive,
            )?;
            
            // Add to fee reserves
            BridgeFeeReserves::<T>::mutate(&platform, |reserve| {
                *reserve = reserve.saturating_add(fee_balance);
            });
            
            // Get a quantum key for encryption
            let next_key_index = NextKeyIndex::<T>::get(&platform);
            let key_pair = BridgeKeys::<T>::get(&platform, next_key_index)
                .ok_or(Error::<T>::NoQuantumKeysAvailable)?;
            
            // Create bridge message
            let message = BridgeMessage {
                source: BlockchainPlatform::Polkadot, // Matrix-Magiq is in Polkadot ecosystem
                target: platform,
                message_type: message_type.clone(),
                sender: BridgeAccountId::Substrate(sender.encode()),
                recipient: target_account,
                payload: payload.clone(),
                quantum_hash: utils::quantum_secure_hash(&payload),
                timestamp: Some(T::BlockNumber::current_time_ms()),
                nonce: next_key_index as u64,
            };
            
            // Store the message with sender
            let message_hash = T::Hashing::hash_of(&message);
            OutboundMessages::<T>::insert(&platform, &message_hash, (message.clone(), sender.clone()));
            
            // Rotate to next key
            NextKeyIndex::<T>::mutate(&platform, |index| {
                *index = (*index + 1) % T::MaxKeysPerChannel::get();
            });
            
            Self::deposit_event(Event::MessageSent {
                platform,
                message_hash,
                message_type,
                sender,
            });
            
            // In a real implementation, we would now relay this message to the target chain
            // using the relayer infrastructure, but that's outside the scope of this pallet
            
            Ok(())
        }

        /// Confirm an outbound message has been processed.
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::confirm_message())]
        pub fn confirm_message(
            origin: OriginFor<T>,
            platform: BlockchainPlatform,
            message_hash: T::Hash,
            transaction_hash: Vec<u8>,
        ) -> DispatchResult {
            T::BridgeAdminOrigin::ensure_origin(origin)?;
            
            ensure!(
                OutboundMessages::<T>::contains_key(&platform, &message_hash),
                Error::<T>::MessageDoesNotExist
            );
            
            // Remove from outbound messages
            OutboundMessages::<T>::remove(&platform, &message_hash);
            
            Self::deposit_event(Event::MessageConfirmed {
                platform,
                message_hash,
                transaction_hash,
            });
            
            Ok(())
        }

        /// Receive and verify a message from another blockchain.
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::receive_message(message.payload.len() as u32))]
        pub fn receive_message(
            origin: OriginFor<T>,
            platform: BlockchainPlatform,
            message: BridgeMessage,
            proof: Vec<u8>,
        ) -> DispatchResult {
            T::BridgeAdminOrigin::ensure_origin(origin)?;
            
            let config = Bridges::<T>::get(&platform).ok_or(Error::<T>::BridgeDoesNotExist)?;
            ensure!(config.active, Error::<T>::BridgeNotActive);
            
            // Verify the message (in a real implementation, this would verify cryptographic proofs)
            // For now, we just check the hash
            let payload_hash = utils::quantum_secure_hash(&message.payload);
            ensure!(payload_hash == message.quantum_hash, Error::<T>::InvalidSignature);
            
            // Store the message for processing
            let message_hash = T::Hashing::hash_of(&message);
            InboundMessages::<T>::insert(&platform, &message_hash, message.clone());
            
            Self::deposit_event(Event::MessageReceived {
                platform,
                message_hash,
                message_type: message.message_type,
            });
            
            // Process community-specific message types
            match message.message_type {
                BridgeMessageType::GovernanceProposal => {
                    // Parse payload as community proposal
                    if let Ok(proposal) = CommunityProposal::decode(&mut &message.payload[..]) {
                        Self::register_community_proposal(platform, proposal)?;
                    }
                },
                BridgeMessageType::SupplyChainUpdate => {
                    // Parse payload as supply chain entry
                    if let Ok(entry) = SupplyChainEntry::decode(&mut &message.payload[..]) {
                        Self::register_supply_chain_entry(entry)?;
                    }
                },
                _ => {
                    // Other message types would be handled by respective modules
                }
            }
            
            Ok(())
        }

        /// Register a new community proposal for sustainable food initiatives.
        #[pallet::call_index(6)]
        #[pallet::weight(T::WeightInfo::register_community_proposal())]
        pub fn register_community_proposal_direct(
            origin: OriginFor<T>,
            platform: BlockchainPlatform,
            title: Vec<u8>,
            description: Vec<u8>,
            funding_amount: u128,
            target_communities: Vec<Vec<u8>>,
            sustainability_metrics: Vec<(Vec<u8>, Vec<u8>)>,
            source_id: Vec<u8>,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            
            ensure!(
                Bridges::<T>::contains_key(&platform),
                Error::<T>::BridgeDoesNotExist
            );
            
            let proposal = CommunityProposal {
                title: title.clone(),
                description,
                funding_amount,
                creator: BridgeAccountId::Substrate(sender.encode()),
                target_communities,
                sustainability_metrics,
                source_chain: BlockchainPlatform::Polkadot,
                source_id: source_id.clone(),
            };
            
            // Ensure proposal doesn't already exist
            ensure!(
                !CommunityProposals::<T>::contains_key(&platform, &source_id),
                Error::<T>::ProposalAlreadyExists
            );
            
            // Store the proposal
            CommunityProposals::<T>::insert(&platform, &source_id, proposal);
            
            Self::deposit_event(Event::CommunityProposalRegistered {
                platform,
                proposal_id: source_id,
                title,
                funding_amount,
            });
            
            Ok(())
        }

        /// Register a supply chain entry for food traceability.
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::register_supply_chain_entry())]
        pub fn register_supply_chain_entry_direct(
            origin: OriginFor<T>,
            product_id: Vec<u8>,
            producer_id: Vec<u8>,
            location: Vec<u8>,
            timestamp: u64,
            certifications: Vec<Vec<u8>>,
            stage: SupplyChainStage,
        ) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            
            // Create verification hash
            let mut data = Vec::new();
            data.extend_from_slice(&product_id);
            data.extend_from_slice(&producer_id);
            data.extend_from_slice(&location);
            data.extend_from_slice(&timestamp.to_be_bytes());
            for cert in &certifications {
                data.extend_from_slice(cert);
            }
            data.extend_from_slice(&(stage as u8).to_be_bytes());
            
            // Apply thrice-nested entropy transformation
            utils::apply_thrice_nested_entropy(&mut data);
            let verification_hash = utils::quantum_secure_hash(&data);
            
            let entry = SupplyChainEntry {
                product_id: product_id.clone(),
                producer_id,
                location,
                timestamp,
                certifications,
                stage,
                verification_hash,
            };
            
            // Check stage progression if entries exist
            let entries = SupplyChainEntries::<T>::get(&product_id);
            if let Some(last_entry) = entries.last() {
                ensure!(
                    last_entry.stage as u8 <= stage as u8,
                    Error::<T>::SupplyChainStageRegression
                );
            }
            
            // Store the entry
            SupplyChainEntries::<T>::mutate(&product_id, |entries| {
                entries.push(entry);
            });
            
            Self::deposit_event(Event::SupplyChainEntryVerified {
                product_id,
                stage,
                verification_hash,
            });
            
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        /// Account ID for the pallet.
        pub fn account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }
        
        /// Generate a quantum key for a bridge.
        fn generate_quantum_key_internal(
            platform: BlockchainPlatform,
            protocol: QuantumProtocol,
            security_level: ThreeNestedSecurityLevel,
        ) -> DispatchResult {
            // Check if we already have too many keys
            let key_count = BridgeKeys::<T>::iter_prefix(&platform).count();
            ensure!(
                key_count < T::MaxKeysPerChannel::get() as usize,
                Error::<T>::TooManyKeys
            );
            
            // Generate a new quantum key pair
            let key_pair = cumulus_primitives_quantum_channel::generate_quantum_keypair(
                protocol, 
                security_level,
            );
            
            // Get the next key index
            let key_index = NextKeyIndex::<T>::get(&platform);
            
            // Store the key
            BridgeKeys::<T>::insert(&platform, key_index, key_pair);
            
            // Update next key index
            NextKeyIndex::<T>::mutate(&platform, |index| {
                *index = (*index + 1) % T::MaxKeysPerChannel::get();
            });
            
            Self::deposit_event(Event::QuantumKeyGenerated {
                platform,
                key_index,
                protocol,
            });
            
            Ok(())
        }
        
        /// Calculate fees for a bridge operation.
        fn calculate_fee(
            config: &BridgeConfig,
            message_type: &BridgeMessageType,
            message_size: u32,
        ) -> Result<u128, Error<T>> {
            match config.fee_model {
                BridgeFeeModel::Fixed(amount) => Ok(amount),
                BridgeFeeModel::Percentage(basis_points) => {
                    // Base fee is 10^6 units, modified by percentage
                    let base_fee = 1_000_000u128;
                    let fee = base_fee.saturating_mul(basis_points as u128) / 10_000;
                    Ok(fee)
                },
                BridgeFeeModel::Tiered(ref tiers) => {
                    // Find the applicable tier based on message size
                    for (threshold, fee) in tiers {
                        if message_size as u128 <= *threshold {
                            return Ok(*fee);
                        }
                    }
                    // If no tier matches, use the last tier
                    tiers.last().map(|(_, fee)| *fee).ok_or(Error::<T>::FeeCalculationError)
                },
                BridgeFeeModel::None => Ok(0),
            }
        }
        
        /// Register a community proposal from a received message.
        fn register_community_proposal(
            platform: BlockchainPlatform,
            proposal: CommunityProposal,
        ) -> DispatchResult {
            // Check if proposal already exists
            ensure!(
                !CommunityProposals::<T>::contains_key(&platform, &proposal.source_id),
                Error::<T>::ProposalAlreadyExists
            );
            
            // Store the proposal
            CommunityProposals::<T>::insert(&platform, &proposal.source_id, proposal.clone());
            
            Self::deposit_event(Event::CommunityProposalRegistered {
                platform,
                proposal_id: proposal.source_id,
                title: proposal.title,
                funding_amount: proposal.funding_amount,
            });
            
            Ok(())
        }
        
        /// Register a supply chain entry from a received message.
        fn register_supply_chain_entry(entry: SupplyChainEntry) -> DispatchResult {
            // Verify the hash
            let mut data = Vec::new();
            data.extend_from_slice(&entry.product_id);
            data.extend_from_slice(&entry.producer_id);
            data.extend_from_slice(&entry.location);
            data.extend_from_slice(&entry.timestamp.to_be_bytes());
            for cert in &entry.certifications {
                data.extend_from_slice(cert);
            }
            data.extend_from_slice(&(entry.stage as u8).to_be_bytes());
            
            // Apply thrice-nested entropy transformation
            utils::apply_thrice_nested_entropy(&mut data);
            let calculated_hash = utils::quantum_secure_hash(&data);
            
            // Verify hash matches
            ensure!(
                calculated_hash == entry.verification_hash,
                Error::<T>::VerificationHashMismatch
            );
            
            // Check stage progression if entries exist
            let entries = SupplyChainEntries::<T>::get(&entry.product_id);
            if let Some(last_entry) = entries.last() {
                ensure!(
                    last_entry.stage as u8 <= entry.stage as u8,
                    Error::<T>::SupplyChainStageRegression
                );
            }
            
            // Store the entry
            SupplyChainEntries::<T>::mutate(&entry.product_id, |entries| {
                entries.push(entry.clone());
            });
            
            Self::deposit_event(Event::SupplyChainEntryVerified {
                product_id: entry.product_id,
                stage: entry.stage,
                verification_hash: entry.verification_hash,
            });
            
            Ok(())
        }
    }
}

// Essential placeholder for PalletId implementation
impl<T: Config> crate::pallet::Pallet<T> {
    pub const PALLET_ID: frame_support::PalletId = frame_support::PalletId(*b"bridgexx");
}

#[impl_trait_for_tuples::impl_for_tuples(30)]
pub trait OnMessageReceived<BlockchainPlatform, AccountId> {
    fn on_message_received(
        source: BlockchainPlatform,
        message_type: BridgeMessageType,
        sender: BridgeAccountId,
        recipient: AccountId,
        payload: Vec<u8>,
    ) -> DispatchResult;
}
