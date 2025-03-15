# Quantum-Secure XCMP Integration Guide

This guide demonstrates how to integrate the quantum-secure XCMP implementation into your Matrix-Magiq parachain. The enhanced Cumulus components provide quantum-resistant communication channels between parachains using the thrice-nested entropy system with iron-nickel-cobalt layers.

## Features

- **Quantum-Resistant Cryptography**: Uses post-quantum cryptographic primitives for enhanced security
- **Thrice-Nested Entropy**: Implements the Matrix-Magiq stereo entropy system with three layers
- **Automatic Key Management**: Generates and rotates quantum keys automatically
- **Compatibility**: Works with existing XCMP infrastructure for seamless integration

## Integration Steps

### 1. Add Dependencies to Your Runtime's Cargo.toml

```toml
[dependencies]
# Quantum-enhanced Cumulus components
cumulus-primitives-quantum-channel = { path = "../../../sdk-fork/cumulus/primitives/quantum-channel", default-features = false }
pallet-quantum-xcmp = { path = "../../../sdk-fork/cumulus/pallets/quantum-xcmp", default-features = false }

# Required quantum dependencies
pqc_kyber = { version = "0.7.1", default-features = false }
pqc_dilithium = { version = "0.2.0", default-features = false }
blake3 = { version = "1.3.3", default-features = false }

[features]
std = [
    # Other std features...
    "cumulus-primitives-quantum-channel/std",
    "pallet-quantum-xcmp/std",
    "blake3/std",
]
```

### 2. Configure the Pallet in Your Runtime

```rust
impl pallet_quantum_xcmp::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type MaxQuantumMessageSize = ConstU32<65536>; // 64 KB
    type MaxKeysPerChannel = ConstU32<20>;
    type ChannelSetupOrigin = EnsureRoot<AccountId>;
    type WeightInfo = pallet_quantum_xcmp::weights::SubstrateWeight<Runtime>;
}
```

### 3. Include the Pallet in Your Runtime's `construct_runtime!` Macro

```rust
construct_runtime!(
    pub enum Runtime where
        Block = Block,
        NodeBlock = opaque::Block,
        UncheckedExtrinsic = UncheckedExtrinsic
    {
        // ... other pallets
        
        // Parachain System and related pallets
        ParachainSystem: cumulus_pallet_parachain_system,
        ParachainInfo: parachain_info,
        
        // Quantum-secure XCMP
        QuantumXcmp: pallet_quantum_xcmp,
        
        // ... other pallets
    }
);
```

### 4. Initialize Quantum Channels

When setting up your parachain, you'll need to establish quantum channels with other parachains you want to communicate with:

```rust
// In your chain specification or initialization code
fn initialize_quantum_channels() {
    // Example: Establish quantum channel with parachain ID 200
    let target_para_id = 200.into();
    let max_message_size = 65536; // 64 KB
    
    // Using the sudo account to establish the channel
    QuantumXcmp::establish_quantum_channel(
        Origin::root(),
        target_para_id,
        max_message_size
    ).expect("Failed to establish quantum channel");
    
    // Generate additional quantum keys with the ThriceNestedStereo protocol
    QuantumXcmp::generate_quantum_key(
        Origin::root(),
        target_para_id,
        QuantumProtocol::ThriceNestedStereo
    ).expect("Failed to generate quantum key");
}
```

### 5. Sending Quantum-Secure Messages

```rust
// Example: Sending a quantum-secure message
fn send_secure_message(target_para_id: ParaId, message: Vec<u8>) -> DispatchResult {
    QuantumXcmp::send_quantum_message(
        Origin::signed(sender_account),
        target_para_id,
        message
    )
}
```

## Advanced Configuration

### Customizing the Thrice-Nested Entropy System

The default implementation uses the iron-nickel-cobalt layers for thrice-nested entropy. You can customize these transformations by modifying the primitive implementation:

```rust
// In your custom implementation:
fn apply_custom_entropy_transformation(data: &mut Vec<u8>) {
    // First layer (iron - Fe) - You can customize the constants
    for i in 0..data.len() {
        data[i] = data[i].wrapping_mul(0x26).wrapping_add(0xFE);
    }
    
    // Second layer (nickel - Ni)
    for i in 0..data.len() {
        let prev = if i > 0 { data[i-1] } else { data[data.len()-1] };
        data[i] = data[i].wrapping_add(prev).wrapping_mul(0x28);
    }
    
    // Third layer (cobalt - Co)
    for i in 0..data.len() {
        let next = if i < data.len()-1 { data[i+1] } else { data[0] };
        data[i] = data[i].wrapping_xor(next).wrapping_add(0x27);
    }
    
    // Optional fourth layer - Matrix-Magiq extended entropy
    // Add your own transformation here
}
```

### Integration with IMRT Enneareal Graph

For Matrix-Magiq's IMRT Enneareal graph with chiral configurations:

```rust
// Example integration with quantum graph functionality
fn apply_enneareal_graph_transformation(state: &mut QuantumState) {
    // Apply chiral transformations specific to IMRT Enneareal graph
    // This would use the quantum graph functionality from your implementation
    
    // Example (implementation will vary):
    for i in 0..state.dimension {
        // Apply Hamiltonian operations specific to IMRT graphs
        // ...
    }
}
```

## Performance Considerations

- Quantum key generation is computationally intensive and should be done during idle blockchain time
- The thrice-nested entropy system adds minimal overhead to message processing
- Monitor available quantum keys and generate new ones before they are depleted

## Security Best Practices

1. Regularly rotate quantum keys even if they haven't been used
2. Use the ThriceNestedStereo protocol for maximum security
3. Limit message sizes to reduce attack surface
4. Implement additional application-level encryption for sensitive data

## Example Runtime Integration

See the NRSH implementation for a complete example of how to integrate quantum-secure XCMP in a production parachain.
