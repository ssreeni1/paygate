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
