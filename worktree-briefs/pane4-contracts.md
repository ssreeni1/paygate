# Pane 4 Brief: Smart Contracts (feat/contracts)

## Context
You are building the Solidity smart contracts for PayGate — on-chain service discovery and (stub) escrow for a micropayment API gateway on Tempo blockchain. The registry is optional but enables agents to discover PayGate-protected APIs on-chain.

## Required Reading
1. `SPEC.md` §6 — Full contract specs including:
   - §6.1 PayGateRegistry.sol (the main contract to build)
   - §6.2 PayGateEscrow.sol (stub only, v0.2)
   - §6.3 MVP Approach (no contract required for MVP)

## What to Build

All files go in `contracts/` directory, structured as a Foundry project.

### 1. `contracts/foundry.toml`
```toml
[profile.default]
src = "src"
out = "out"
libs = ["lib"]
solc = "0.8.24"
optimizer = true
optimizer_runs = 200

[profile.default.fmt]
line_length = 100
tab_width = 4
```

### 2. Initialize Foundry dependencies
Run `forge init --no-commit --no-git` or manually set up:
- `forge install foundry-rs/forge-std --no-commit` (for test utilities)
- Create `remappings.txt`: `forge-std/=lib/forge-std/src/`

### 3. `contracts/src/interfaces/ITIP20.sol`
```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/**
 * @title ITIP20
 * @notice Interface for TIP-20 tokens on Tempo blockchain.
 * TIP-20 extends ERC-20 with memo support for payment binding.
 */
interface ITIP20 {
    // Standard ERC-20
    function name() external view returns (string memory);
    function symbol() external view returns (string memory);
    function decimals() external view returns (uint8);
    function totalSupply() external view returns (uint256);
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
    function approve(address spender, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);

    // TIP-20 extension: transfer with memo for payment binding
    function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);

    // Events
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event TransferWithMemo(address indexed from, address indexed to, uint256 value, bytes32 memo);
}
```

### 4. `contracts/src/PayGateRegistry.sol`
Implement EXACTLY as specified in SPEC §6.1, but with these additions:
- Add NatSpec documentation
- Add input validation (provider cannot be zero address, price can be zero for free APIs)
- Add a `getService(bytes32 serviceId)` view function
- Add a `isActive(bytes32 serviceId)` view function
- Add a `ServiceUpdated` event for price changes
- Add a `ServiceDeactivated` event

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/**
 * @title PayGateRegistry
 * @notice On-chain service discovery for PayGate-protected APIs.
 * Providers register their API services with pricing info.
 * Agents can discover available services and their prices.
 */
contract PayGateRegistry {
    struct Service {
        address provider;
        uint256 pricePerRequest;    // stablecoin base units (6 decimals for USDC)
        address acceptedToken;       // TIP-20 token address
        bool active;
        string metadataUri;          // URL to pricing manifest (JSON)
    }

    mapping(bytes32 => Service) public services;

    event ServiceRegistered(bytes32 indexed serviceId, address indexed provider, uint256 price);
    event ServiceUpdated(bytes32 indexed serviceId, uint256 oldPrice, uint256 newPrice);
    event ServiceDeactivated(bytes32 indexed serviceId);

    error NotProvider();
    error ZeroAddress();
    error ServiceNotFound();

    function registerService(
        string calldata name,
        uint256 pricePerRequest,
        address acceptedToken,
        string calldata metadataUri
    ) external returns (bytes32 serviceId) {
        if (acceptedToken == address(0)) revert ZeroAddress();

        serviceId = keccak256(abi.encodePacked(msg.sender, name));
        services[serviceId] = Service({
            provider: msg.sender,
            pricePerRequest: pricePerRequest,
            acceptedToken: acceptedToken,
            active: true,
            metadataUri: metadataUri
        });
        emit ServiceRegistered(serviceId, msg.sender, pricePerRequest);
    }

    function updatePrice(bytes32 serviceId, uint256 newPrice) external {
        if (services[serviceId].provider != msg.sender) revert NotProvider();
        uint256 oldPrice = services[serviceId].pricePerRequest;
        services[serviceId].pricePerRequest = newPrice;
        emit ServiceUpdated(serviceId, oldPrice, newPrice);
    }

    function deactivate(bytes32 serviceId) external {
        if (services[serviceId].provider != msg.sender) revert NotProvider();
        services[serviceId].active = false;
        emit ServiceDeactivated(serviceId);
    }

    function getService(bytes32 serviceId) external view returns (Service memory) {
        if (services[serviceId].provider == address(0)) revert ServiceNotFound();
        return services[serviceId];
    }

    function isActive(bytes32 serviceId) external view returns (bool) {
        return services[serviceId].active;
    }
}
```

### 5. `contracts/src/PayGateEscrow.sol` — Stub for v0.2
```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/**
 * @title PayGateEscrow
 * @notice Escrow for refund-eligible payments. STUB — implementation deferred to v0.2.
 *
 * Design requirements (from SPEC §6.2):
 * - Gateway is the authorized releaser (signs release after successful upstream response)
 * - Provider can release manually as fallback
 * - Payer can claim refund unilaterally after escrow expiry
 * - release() callable only by gateway or provider
 * - refund() callable only by payer after expiresAt
 */
contract PayGateEscrow {
    // TODO: v0.2 implementation
    // See SPEC.md §6.2 for full design requirements
}
```

### 6. `contracts/test/PayGateRegistry.t.sol`
Comprehensive Foundry tests:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/PayGateRegistry.sol";

contract PayGateRegistryTest is Test {
    PayGateRegistry registry;
    address provider1 = address(0x1);
    address provider2 = address(0x2);
    address token = address(0xUSDC); // mock token address

    function setUp() public {
        registry = new PayGateRegistry();
    }

    // Test: Register a service and verify all fields
    // Test: serviceId = keccak256(abi.encodePacked(msg.sender, name))
    // Test: Update price — only provider can
    // Test: Update price — non-provider reverts with NotProvider()
    // Test: Deactivate — only provider can
    // Test: Deactivate — non-provider reverts
    // Test: Register multiple services from same provider
    // Test: Same name from different providers → different serviceIds
    // Test: getService returns correct data
    // Test: getService reverts for non-existent service
    // Test: isActive returns correct state
    // Test: ServiceRegistered event emitted with correct params
    // Test: ServiceUpdated event emitted with old and new price
    // Test: ServiceDeactivated event emitted
    // Test: Zero address token reverts
    // Test: Price of 0 is valid (free APIs)
    // Test: Register, deactivate, check isActive = false
}
```

Write ALL of these tests with actual assertions, not just comments.

## Running Tests
```bash
cd contracts
forge install foundry-rs/forge-std --no-commit
forge test -vv
```

ALL tests must pass.

## Commit
Commit with descriptive message on feat/contracts branch.
