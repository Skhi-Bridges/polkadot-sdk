//! # Quantum XCMP Pallet
//!
//! A pallet that provides quantum-secure cross-chain message passing for Cumulus parachains.
//! This pallet enhances the standard XCMP with quantum-resistant cryptography and
//! the Matrix-Magiq thrice-nested entropy system.

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use codec::{Decode, Encode};
    use cumulus_pallet_parachain_system::RelayNumberStrictlyIncreases;
    use cumulus_primitives_core::{
        MessageSendError, ParaId, RelayChainBlockNumber, XcmpMessageHandler, XcmpMessageSource,
    };
    use cumulus_primitives_quantum_channel::{
        QuantumChannelInfo, QuantumChannelKey, QuantumChannelStatus, QuantumProtocol,
        QuantumSecureXcmpMessage, QuantumState, QuantumXcmpMessageHandler, quantum_secure_hash,
    };
    use frame_support::{
        pallet_prelude::*,
        traits::{Get, OneSessionHandler},
        weights::Weight,
    };
    use frame_system::pallet_prelude::*;
    use sp_core::crypto::KeyTypeId;
    use sp_runtime::{traits::Hash, FixedU128, RuntimeDebug};
    use sp_std::prelude::*;

    /// Key type for quantum channel keys.
    pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"qxcm");

    /// Configuration trait for the quantum XCMP pallet.
    #[pallet::config]
    pub trait Config: frame_system::Config + cumulus_pallet_parachain_system::Config {
        /// The overarching event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The maximum size of a quantum-secure message.
        #[pallet::constant]
        type MaxQuantumMessageSize: Get<u32>;

        /// The maximum number of quantum keys to store per channel.
        #[pallet::constant]
        type MaxKeysPerChannel: Get<u32>;

        /// The origin that is allowed to establish new quantum channels.
        type ChannelSetupOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// Weight information for the extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Quantum channel information storage - maps ParaId to QuantumChannelInfo.
    #[pallet::storage]
    pub type QuantumChannels<T: Config> = StorageMap<_, Blake2_128Concat, ParaId, QuantumChannelInfo>;

    /// Quantum keys storage - maps (ParaId, key_id) to QuantumChannelKey.
    #[pallet::storage]
    pub type QuantumKeys<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, ParaId, Blake2_128Concat, [u8; 32], QuantumChannelKey>;

    /// Pending outbound quantum messages.
    #[pallet::storage]
    pub type OutboundQuantumMessages<T: Config> =
        StorageMap<_, Blake2_128Concat, ParaId, Vec<QuantumSecureXcmpMessage>, ValueQuery>;

    /// Delivery fee factor for quantum messages.
    #[pallet::storage]
    pub type QuantumDeliveryFeeFactor<T: Config> = StorageValue<_, FixedU128, ValueQuery, InitialFeeFactor>;

    /// Default initial fee factor.
    #[pallet::type_value]
    pub fn InitialFeeFactor() -> FixedU128 {
        FixedU128::from_rational(1, 1)
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A quantum channel has been established with another parachain.
        QuantumChannelEstablished {
            para_id: ParaId,
            status: QuantumChannelStatus,
        },
        /// A new quantum key has been generated.
        QuantumKeyGenerated {
            para_id: ParaId,
            protocol: QuantumProtocol,
        },
        /// A quantum-secure message has been sent.
        QuantumMessageSent {
            para_id: ParaId,
            message_size: u32,
        },
        /// A quantum-secure message has been received.
        QuantumMessageReceived {
            from_para_id: ParaId,
            message_size: u32,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The quantum channel does not exist.
        QuantumChannelNotFound,
        /// The quantum channel is not ready.
        QuantumChannelNotReady,
        /// No quantum keys available for the channel.
        NoQuantumKeysAvailable,
        /// Message is too large for the quantum channel.
        MessageTooLarge,
        /// Failed to send message to the destination parachain.
        MessageSendFailed,
        /// Maximum number of keys per channel reached.
        MaxKeysPerChannelReached,
        /// Failed to generate quantum entropy.
        QuantumEntropyGenerationFailed,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Process outbound quantum messages at the end of each block.
        fn on_finalize(_n: BlockNumberFor<T>) {
            Self::process_outbound_quantum_messages();
        }

        /// Generate new quantum keys for channels with low key count.
        fn on_idle(_n: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            let mut used_weight = Weight::zero();
            let weight_per_key = T::WeightInfo::generate_quantum_key();

            // Only proceed if we have enough weight to generate at least one key
            if remaining_weight < weight_per_key {
                return used_weight;
            }

            // Find channels with low key count and generate new keys
            QuantumChannels::<T>::iter().for_each(|(para_id, channel_info)| {
                if used_weight + weight_per_key > remaining_weight {
                    return;
                }

                if channel_info.status == QuantumChannelStatus::Ready && 
                   channel_info.available_keys < T::MaxKeysPerChannel::get() / 2 {
                    // Generate a new quantum key
                    if let Ok(_) = Self::do_generate_quantum_key(&para_id, QuantumProtocol::ThriceNestedStereo) {
                        used_weight = used_weight.saturating_add(weight_per_key);
                    }
                }
            });

            used_weight
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Establish a quantum-secure channel with another parachain.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::establish_quantum_channel())]
        pub fn establish_quantum_channel(
            origin: OriginFor<T>,
            target: ParaId,
            max_message_size: u32,
        ) -> DispatchResult {
            T::ChannelSetupOrigin::ensure_origin(origin)?;

            ensure!(
                max_message_size <= T::MaxQuantumMessageSize::get(),
                Error::<T>::MessageTooLarge
            );

            // Create the quantum channel info
            let channel_info = QuantumChannelInfo {
                para_id: target,
                status: QuantumChannelStatus::EstablishingKeys,
                max_message_size,
                available_keys: 0,
            };

            // Store the channel info
            QuantumChannels::<T>::insert(target, channel_info.clone());

            // Generate initial quantum keys
            for _ in 0..3 {
                let _ = Self::do_generate_quantum_key(&target, QuantumProtocol::ThriceNestedStereo);
            }

            // Update channel status to Ready if we have at least one key
            if let Some(mut info) = QuantumChannels::<T>::get(target) {
                if info.available_keys > 0 {
                    info.status = QuantumChannelStatus::Ready;
                    QuantumChannels::<T>::insert(target, info.clone());
                }
            }

            // Emit the event
            Self::deposit_event(Event::QuantumChannelEstablished {
                para_id: target,
                status: QuantumChannelStatus::EstablishingKeys,
            });

            Ok(())
        }

        /// Generate a new quantum key for a channel.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::generate_quantum_key())]
        pub fn generate_quantum_key(
            origin: OriginFor<T>,
            target: ParaId,
            protocol: QuantumProtocol,
        ) -> DispatchResult {
            T::ChannelSetupOrigin::ensure_origin(origin)?;

            // Ensure the channel exists
            ensure!(QuantumChannels::<T>::contains_key(target), Error::<T>::QuantumChannelNotFound);

            // Generate the quantum key
            Self::do_generate_quantum_key(&target, protocol)?;

            Ok(())
        }

        /// Send a quantum-secure message to another parachain.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::send_quantum_message(data.len() as u32))]
        pub fn send_quantum_message(
            origin: OriginFor<T>,
            target: ParaId,
            data: Vec<u8>,
        ) -> DispatchResult {
            ensure_signed(origin)?;

            let data_len = data.len() as u32;

            // Ensure the channel exists and is ready
            let channel_info = QuantumChannels::<T>::get(target)
                .ok_or(Error::<T>::QuantumChannelNotFound)?;

            ensure!(
                channel_info.status == QuantumChannelStatus::Ready,
                Error::<T>::QuantumChannelNotReady
            );

            // Ensure the message is not too large
            ensure!(
                data_len <= channel_info.max_message_size,
                Error::<T>::MessageTooLarge
            );

            // Ensure we have at least one quantum key available
            ensure!(channel_info.available_keys > 0, Error::<T>::NoQuantumKeysAvailable);

            // Find an available quantum key
            let mut key_id = None;
            let mut found_key = None;

            QuantumKeys::<T>::iter_prefix_values(target).for_each(|key| {
                if key_id.is_none() {
                    key_id = Some(key.id);
                    found_key = Some(key);
                }
            });

            let key = found_key.ok_or(Error::<T>::NoQuantumKeysAvailable)?;

            // Encrypt the message using the quantum key
            let encrypted_data = Self::encrypt_with_quantum_key(&data, &key);

            // Generate quantum tag for verification
            let quantum_tag = quantum_secure_hash(&[&encrypted_data[..], &key.key_material[..]].concat());

            // Create the quantum-secure message
            let quantum_message = QuantumSecureXcmpMessage {
                destination: target,
                encrypted_data,
                quantum_tag,
            };

            // Queue the message for sending
            OutboundQuantumMessages::<T>::mutate(target, |messages| {
                messages.push(quantum_message);
            });

            // Remove the used key
            if let Some(id) = key_id {
                QuantumKeys::<T>::remove(target, id);
            }

            // Update the channel info to reflect one less key
            let mut updated_info = channel_info;
            updated_info.available_keys = updated_info.available_keys.saturating_sub(1);
            QuantumChannels::<T>::insert(target, updated_info);

            // Emit the event
            Self::deposit_event(Event::QuantumMessageSent {
                para_id: target,
                message_size: data_len,
            });

            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        /// Process all outbound quantum messages.
        fn process_outbound_quantum_messages() -> Weight {
            let mut total_weight = Weight::zero();
            let weight_per_message = T::WeightInfo::send_quantum_message(0);

            OutboundQuantumMessages::<T>::iter().for_each(|(para_id, messages)| {
                for message in messages {
                    // Send the message via XCMP
                    let _ = cumulus_pallet_parachain_system::Pallet::<T>::send_upward_message(
                        message.encrypted_data.clone(),
                    );

                    total_weight = total_weight.saturating_add(weight_per_message);
                }

                // Clear the outbound messages for this parachain
                OutboundQuantumMessages::<T>::remove(para_id);
            });

            total_weight
        }

        /// Generate a quantum key for a channel.
        fn do_generate_quantum_key(
            para_id: &ParaId,
            protocol: QuantumProtocol,
        ) -> Result<QuantumChannelKey, DispatchError> {
            // Get the channel info
            let mut channel_info = QuantumChannels::<T>::get(para_id)
                .ok_or(Error::<T>::QuantumChannelNotFound)?;

            // Check if we've reached the max keys per channel
            ensure!(
                channel_info.available_keys < T::MaxKeysPerChannel::get(),
                Error::<T>::MaxKeysPerChannelReached
            );

            // Generate quantum state based on the protocol
            let dimension = match protocol {
                QuantumProtocol::BB84 => 2,
                QuantumProtocol::E91 => 3,
                QuantumProtocol::ThriceNestedStereo => 4, // For thrice-nested entropy
            };

            let mut quantum_state = QuantumState::new(dimension);

            // Apply Hadamard gates for superposition
            for i in 0..dimension.min(2) {
                quantum_state.apply_hadamard(i);
            }

            // Generate entropy from the quantum state
            let entropy = quantum_state.to_entropy_bytes();
            ensure!(!entropy.is_empty(), Error::<T>::QuantumEntropyGenerationFailed);

            // Create quantum signature
            let quantum_signature = entropy[..32].to_vec();

            // Create the key ID
            let key_id = quantum_secure_hash(&entropy);

            // Create the quantum key
            let key = QuantumChannelKey {
                id: key_id,
                key_material: entropy,
                protocol,
                quantum_signature,
            };

            // Store the key
            QuantumKeys::<T>::insert(para_id, key_id, key.clone());

            // Update the channel info
            channel_info.available_keys = channel_info.available_keys.saturating_add(1);
            if channel_info.status == QuantumChannelStatus::EstablishingKeys {
                channel_info.status = QuantumChannelStatus::Ready;
            }
            QuantumChannels::<T>::insert(para_id, channel_info);

            // Emit the event
            Self::deposit_event(Event::QuantumKeyGenerated {
                para_id: *para_id,
                protocol,
            });

            Ok(key)
        }

        /// Encrypt data with a quantum key using thrice-nested entropy.
        fn encrypt_with_quantum_key(data: &[u8], key: &QuantumChannelKey) -> Vec<u8> {
            let mut result = data.to_vec();
            let key_material = &key.key_material;

            // Apply encryption using the quantum key
            for i in 0..result.len() {
                let key_idx = i % key_material.len();
                result[i] = result[i] ^ key_material[key_idx];
            }

            // Apply thrice-nested entropy transformation (iron-nickel-cobalt layers)
            // Layer 1 (iron)
            for i in 0..result.len() {
                result[i] = result[i].wrapping_mul(0x26).wrapping_add(0xFE);
            }

            // Layer 2 (nickel)
            for i in 0..result.len() {
                let prev = if i > 0 { result[i-1] } else { result[result.len()-1] };
                result[i] = result[i].wrapping_add(prev).wrapping_mul(0x28);
            }

            // Layer 3 (cobalt)
            for i in 0..result.len() {
                let next = if i < result.len()-1 { result[i+1] } else { result[0] };
                result[i] = result[i].wrapping_xor(next).wrapping_add(0x27);
            }

            result
        }

        /// Decrypt data with a quantum key.
        fn decrypt_with_quantum_key(encrypted_data: &[u8], key: &QuantumChannelKey) -> Vec<u8> {
            let mut result = encrypted_data.to_vec();
            let key_material = &key.key_material;

            // Reverse the thrice-nested entropy transformation
            // Reverse Layer 3 (cobalt)
            for i in (0..result.len()).rev() {
                let next = if i < result.len()-1 { result[i+1] } else { result[0] };
                result[i] = result[i].wrapping_sub(0x27).wrapping_xor(next);
            }

            // Reverse Layer 2 (nickel)
            for i in (0..result.len()).rev() {
                let prev = if i > 0 { result[i-1] } else { result[result.len()-1] };
                result[i] = result[i].wrapping_div(0x28).wrapping_sub(prev);
            }

            // Reverse Layer 1 (iron)
            for i in (0..result.len()).rev() {
                result[i] = result[i].wrapping_sub(0xFE).wrapping_div(0x26);
            }

            // Apply decryption using the quantum key (XOR is symmetric)
            for i in 0..result.len() {
                let key_idx = i % key_material.len();
                result[i] = result[i] ^ key_material[key_idx];
            }

            result
        }
    }

    impl<T: Config> QuantumXcmpMessageHandler for Pallet<T> {
        fn handle_quantum_xcmp_message(
            source: ParaId,
            message: QuantumSecureXcmpMessage,
        ) -> Result<(), MessageSendError> {
            // Check if we have a quantum channel with the source parachain
            let channel_info = QuantumChannels::<T>::get(source)
                .ok_or(MessageSendError::ChannelClosed)?;

            // Ensure the channel is ready
            if channel_info.status != QuantumChannelStatus::Ready {
                return Err(MessageSendError::ChannelClosed);
            }

            // Find a key to decrypt the message
            let mut found_key = None;

            QuantumKeys::<T>::iter_prefix_values(source).for_each(|key| {
                if found_key.is_none() {
                    // Calculate expected quantum tag
                    let expected_tag = quantum_secure_hash(
                        &[&message.encrypted_data[..], &key.key_material[..]].concat()
                    );

                    // If tags match, use this key
                    if expected_tag == message.quantum_tag {
                        found_key = Some(key);
                    }
                }
            });

            let key = found_key.ok_or(MessageSendError::NotInChannelMask)?;

            // Decrypt the message
            let decrypted_data = Self::decrypt_with_quantum_key(&message.encrypted_data, &key);

            // Remove the used key
            QuantumKeys::<T>::remove(source, key.id);

            // Update the channel info
            let mut updated_info = channel_info;
            updated_info.available_keys = updated_info.available_keys.saturating_sub(1);
            QuantumChannels::<T>::insert(source, updated_info);

            // Emit event
            Self::deposit_event(Event::QuantumMessageReceived {
                from_para_id: source,
                message_size: decrypted_data.len() as u32,
            });

            Ok(())
        }

        fn establish_quantum_channel(target: ParaId) -> Result<QuantumChannelInfo, MessageSendError> {
            // Get or create the quantum channel
            let channel_info = if let Some(info) = QuantumChannels::<T>::get(target) {
                info
            } else {
                // Create a new channel with default settings
                let info = QuantumChannelInfo {
                    para_id: target,
                    status: QuantumChannelStatus::EstablishingKeys,
                    max_message_size: T::MaxQuantumMessageSize::get(),
                    available_keys: 0,
                };

                // Store the channel info
                QuantumChannels::<T>::insert(target, info.clone());

                // Generate initial quantum keys
                for _ in 0..3 {
                    let _ = Self::do_generate_quantum_key(&target, QuantumProtocol::ThriceNestedStereo);
                }

                info
            };

            Ok(channel_info)
        }

        fn generate_quantum_key(
            target: ParaId,
            protocol: QuantumProtocol,
        ) -> Result<QuantumChannelKey, MessageSendError> {
            Self::do_generate_quantum_key(&target, protocol).map_err(|_| MessageSendError::TemporarilyUnavailable)
        }
    }
}

/// Weight functions needed for pallet_quantum_xcmp.
pub trait WeightInfo {
    fn establish_quantum_channel() -> Weight;
    fn generate_quantum_key() -> Weight;
    fn send_quantum_message(s: u32) -> Weight;
}

impl WeightInfo for () {
    fn establish_quantum_channel() -> Weight {
        Weight::from_parts(10_000_000, 0)
    }

    fn generate_quantum_key() -> Weight {
        Weight::from_parts(20_000_000, 0)
    }

    fn send_quantum_message(_s: u32) -> Weight {
        Weight::from_parts(15_000_000, 0)
    }
}
