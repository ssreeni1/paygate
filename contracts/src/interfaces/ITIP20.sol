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
