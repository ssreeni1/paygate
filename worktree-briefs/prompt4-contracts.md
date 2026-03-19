Read worktree-briefs/pane4-contracts.md in full — it contains your complete build brief with the full contract code, interface spec, and all test cases to implement.

Before writing any code, read:
- SPEC.md §6 (contract specs — §6.1 Registry, §6.2 Escrow stub, §6.3 MVP approach)

You are on branch feat/contracts in a git worktree at ~/projects/paygate-wt-contracts.

Follow the brief exactly. Set up a Foundry project in contracts/ and build:
1. foundry.toml — solc 0.8.24+, optimizer enabled
2. src/interfaces/ITIP20.sol — TIP-20 interface (ERC-20 + transferWithMemo + events)
3. src/PayGateRegistry.sol — on-chain service discovery with registerService, updatePrice, deactivate, getService, isActive, custom errors, events
4. src/PayGateEscrow.sol — stub with design requirement comments for v0.2
5. test/PayGateRegistry.t.sol — ALL test cases from the brief with REAL assertions:
   - Register service and verify all fields stored correctly
   - serviceId computation matches keccak256(abi.encodePacked(sender, name))
   - Update price — only provider can, emits ServiceUpdated event
   - Deactivate — only provider can, emits ServiceDeactivated event
   - Non-provider update/deactivate reverts with NotProvider()
   - Multiple services from same provider
   - Same name from different providers → different serviceIds
   - getService reverts for non-existent service
   - Zero address token reverts
   - Price of 0 is valid (free APIs)
   - Event parameter verification

Run `cd contracts && forge install foundry-rs/forge-std --no-commit && forge test -vv` to verify ALL tests pass. Commit your work with a descriptive message when done.
