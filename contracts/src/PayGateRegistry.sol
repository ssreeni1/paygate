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
        uint256 pricePerRequest; // stablecoin base units (6 decimals for USDC)
        address acceptedToken; // TIP-20 token address
        bool active;
        string metadataUri; // URL to pricing manifest (JSON)
    }

    mapping(bytes32 => Service) public services;

    event ServiceRegistered(bytes32 indexed serviceId, address indexed provider, uint256 price);
    event ServiceUpdated(bytes32 indexed serviceId, uint256 oldPrice, uint256 newPrice);
    event ServiceDeactivated(bytes32 indexed serviceId);

    error NotProvider();
    error ZeroAddress();
    error ServiceNotFound();

    /**
     * @notice Register a new API service with pricing info.
     * @param name Human-readable service name (used with msg.sender to derive serviceId)
     * @param pricePerRequest Price per request in token base units (0 is valid for free APIs)
     * @param acceptedToken TIP-20 token address for payments
     * @param metadataUri URL to pricing manifest JSON
     * @return serviceId Unique identifier derived from keccak256(abi.encodePacked(msg.sender, name))
     */
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

    /**
     * @notice Update the price of a registered service. Only the provider can call this.
     * @param serviceId The service to update
     * @param newPrice New price per request in token base units
     */
    function updatePrice(bytes32 serviceId, uint256 newPrice) external {
        if (services[serviceId].provider != msg.sender) revert NotProvider();
        uint256 oldPrice = services[serviceId].pricePerRequest;
        services[serviceId].pricePerRequest = newPrice;
        emit ServiceUpdated(serviceId, oldPrice, newPrice);
    }

    /**
     * @notice Deactivate a registered service. Only the provider can call this.
     * @param serviceId The service to deactivate
     */
    function deactivate(bytes32 serviceId) external {
        if (services[serviceId].provider != msg.sender) revert NotProvider();
        services[serviceId].active = false;
        emit ServiceDeactivated(serviceId);
    }

    /**
     * @notice Get full service details. Reverts if service does not exist.
     * @param serviceId The service to query
     * @return The Service struct
     */
    function getService(bytes32 serviceId) external view returns (Service memory) {
        if (services[serviceId].provider == address(0)) revert ServiceNotFound();
        return services[serviceId];
    }

    /**
     * @notice Check if a service is currently active.
     * @param serviceId The service to query
     * @return True if the service is active
     */
    function isActive(bytes32 serviceId) external view returns (bool) {
        return services[serviceId].active;
    }
}
