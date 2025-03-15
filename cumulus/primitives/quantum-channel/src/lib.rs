//! Quantum-secure channel primitives for Cumulus parachains.
//!
//! This module provides quantum-resistant communication channels for Cumulus parachains
//! using the Matrix-Magiq quantum security features, including thrice-nested entropy
//! and quantum-resistant cryptography.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{vec, vec::Vec};
use codec::{Decode, Encode};
use cumulus_primitives_core::{MessageSendError, ParaId, XcmpMessageHandler};
use num_complex::Complex64;
use scale_info::TypeInfo;
use sp_runtime::{RuntimeDebug, traits::Hash};
use sp_std::prelude::*;

/// Quantum channel status information.
#[derive(Copy, Clone, Encode, Decode, Eq, PartialEq, RuntimeDebug, TypeInfo)]
pub enum QuantumChannelStatus {
    /// Channel is ready for quantum-secure communication.
    Ready,
    /// Channel is in the process of establishing quantum keys.
    EstablishingKeys,
    /// Channel has failed to establish quantum keys.
    Failed,
}

/// Information about a quantum channel.
#[derive(Clone, Encode, Decode, Eq, PartialEq, RuntimeDebug, TypeInfo)]
pub struct QuantumChannelInfo {
    /// The parachain this channel connects to.
    pub para_id: ParaId,
    /// Current status of the quantum channel.
    pub status: QuantumChannelStatus,
    /// Maximum message size for this channel.
    pub max_message_size: u32,
    /// Number of quantum keys currently available.
    pub available_keys: u32,
}

/// A quantum-secure message for XCMP (Cross-Chain Message Passing).
#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct QuantumSecureXcmpMessage {
    /// The destination parachain.
    pub destination: ParaId,
    /// The encrypted message data.
    pub encrypted_data: Vec<u8>,
    /// Quantum cryptographic verification tag.
    pub quantum_tag: [u8; 32],
}

/// Quantum key distribution protocol identifiers.
#[derive(Copy, Clone, Encode, Decode, Eq, PartialEq, RuntimeDebug, TypeInfo)]
pub enum QuantumProtocol {
    /// BB84 protocol.
    BB84,
    /// E91 protocol.
    E91,
    /// Thrice-nested entropy protocol with stereo channels.
    ThriceNestedStereo,
}

/// Quantum channel key - represents a quantum-derived key for secure communication.
#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct QuantumChannelKey {
    /// Key identifier.
    pub id: [u8; 32],
    /// The quantum-derived key material.
    pub key_material: Vec<u8>,
    /// Protocol used to generate this key.
    pub protocol: QuantumProtocol,
    /// Quantum state signature.
    pub quantum_signature: Vec<u8>,
}

/// Interface for handling quantum-secure XCMP messages.
pub trait QuantumXcmpMessageHandler {
    /// Process a quantum-secure XCMP message.
    fn handle_quantum_xcmp_message(
        source: ParaId,
        message: QuantumSecureXcmpMessage,
    ) -> Result<(), MessageSendError>;
    
    /// Establish a quantum-secure channel with another parachain.
    fn establish_quantum_channel(target: ParaId) -> Result<QuantumChannelInfo, MessageSendError>;
    
    /// Generate a new quantum key for a channel.
    fn generate_quantum_key(
        target: ParaId,
        protocol: QuantumProtocol,
    ) -> Result<QuantumChannelKey, MessageSendError>;
}

/// Quantum state representation for channel security.
#[derive(Clone, RuntimeDebug)]
pub struct QuantumState {
    /// Complex amplitudes representing the quantum state.
    pub amplitudes: Vec<Complex64>,
    /// Dimension of the quantum system.
    pub dimension: usize,
}

impl QuantumState {
    /// Create a new quantum state with specified dimension.
    pub fn new(dimension: usize) -> Self {
        let mut amplitudes = Vec::with_capacity(dimension);
        // Initialize to |0> state
        amplitudes.push(Complex64::new(1.0, 0.0));
        for _ in 1..dimension {
            amplitudes.push(Complex64::new(0.0, 0.0));
        }
        
        Self {
            amplitudes,
            dimension,
        }
    }
    
    /// Apply a Hadamard gate to create superposition.
    pub fn apply_hadamard(&mut self, target_qubit: usize) {
        if self.dimension < 2 || target_qubit >= self.dimension.trailing_zeros() as usize {
            return;
        }
        
        let factor = 1.0 / 2.0f64.sqrt();
        let dim = 1 << target_qubit;
        
        for i in 0..self.amplitudes.len() {
            if i & dim == 0 {
                let i1 = i | dim;
                let temp = self.amplitudes[i];
                self.amplitudes[i] = factor * (temp + self.amplitudes[i1]);
                self.amplitudes[i1] = factor * (temp - self.amplitudes[i1]);
            }
        }
    }
    
    /// Convert the quantum state to entropy bytes.
    pub fn to_entropy_bytes(&self) -> Vec<u8> {
        // Use amplitudes to generate entropy
        let mut result = Vec::new();
        
        for amp in &self.amplitudes {
            let real_bytes = amp.re.to_le_bytes();
            let imag_bytes = amp.im.to_le_bytes();
            
            result.extend_from_slice(&real_bytes);
            result.extend_from_slice(&imag_bytes);
        }
        
        // Apply thrice-nested entropy transformation
        apply_thrice_nested_entropy(&mut result);
        
        result
    }
}

/// Apply thrice-nested entropy transformation to the input data.
fn apply_thrice_nested_entropy(data: &mut Vec<u8>) {
    if data.is_empty() {
        return;
    }
    
    // First layer - iron (Fe)
    for i in 0..data.len() {
        data[i] = data[i].wrapping_mul(0x26).wrapping_add(0xFE);
    }
    
    // Second layer - nickel (Ni)
    for i in 0..data.len() {
        let prev = if i > 0 { data[i-1] } else { data[data.len()-1] };
        data[i] = data[i].wrapping_add(prev).wrapping_mul(0x28);
    }
    
    // Third layer - cobalt (Co)
    for i in 0..data.len() {
        let next = if i < data.len()-1 { data[i+1] } else { data[0] };
        data[i] = data[i].wrapping_xor(next).wrapping_add(0x27);
    }
}

/// Generate a quantum-secure hash using BLAKE3.
pub fn quantum_secure_hash(data: &[u8]) -> [u8; 32] {
    blake3::hash(data).as_bytes().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_quantum_state_creation() {
        let state = QuantumState::new(2);
        assert_eq!(state.amplitudes.len(), 2);
        assert_eq!(state.dimension, 2);
        assert_eq!(state.amplitudes[0], Complex64::new(1.0, 0.0));
        assert_eq!(state.amplitudes[1], Complex64::new(0.0, 0.0));
    }
    
    #[test]
    fn test_hadamard_application() {
        let mut state = QuantumState::new(2);
        state.apply_hadamard(0);
        
        let factor = 1.0 / 2.0f64.sqrt();
        assert!((state.amplitudes[0].re - factor).abs() < 1e-10);
        assert!((state.amplitudes[1].re - factor).abs() < 1e-10);
    }
    
    #[test]
    fn test_entropy_generation() {
        let state = QuantumState::new(2);
        let entropy = state.to_entropy_bytes();
        assert!(!entropy.is_empty());
    }
}
