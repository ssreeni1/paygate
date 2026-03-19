// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/PayGateRegistry.sol";

contract PayGateRegistryTest is Test {
    PayGateRegistry registry;
    address provider1 = address(0x1);
    address provider2 = address(0x2);
    address token = address(0xA11CE); // mock token address

    function setUp() public {
        registry = new PayGateRegistry();
    }

    function test_RegisterServiceAndVerifyAllFields() public {
        vm.prank(provider1);
        bytes32 serviceId =
            registry.registerService("my-api", 1000, token, "https://example.com/manifest.json");

        PayGateRegistry.Service memory svc = registry.getService(serviceId);
        assertEq(svc.provider, provider1);
        assertEq(svc.pricePerRequest, 1000);
        assertEq(svc.acceptedToken, token);
        assertTrue(svc.active);
        assertEq(svc.metadataUri, "https://example.com/manifest.json");
    }

    function test_ServiceIdMatchesKeccak256() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        bytes32 expected = keccak256(abi.encodePacked(provider1, "my-api"));
        assertEq(serviceId, expected);
    }

    function test_UpdatePrice_OnlyProvider() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        vm.prank(provider1);
        registry.updatePrice(serviceId, 2000);

        PayGateRegistry.Service memory svc = registry.getService(serviceId);
        assertEq(svc.pricePerRequest, 2000);
    }

    function test_UpdatePrice_EmitsServiceUpdated() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        vm.expectEmit(true, false, false, true);
        emit PayGateRegistry.ServiceUpdated(serviceId, 1000, 2000);

        vm.prank(provider1);
        registry.updatePrice(serviceId, 2000);
    }

    function test_UpdatePrice_NonProviderReverts() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        vm.prank(provider2);
        vm.expectRevert(PayGateRegistry.NotProvider.selector);
        registry.updatePrice(serviceId, 2000);
    }

    function test_Deactivate_OnlyProvider() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        vm.prank(provider1);
        registry.deactivate(serviceId);

        assertFalse(registry.isActive(serviceId));
    }

    function test_Deactivate_EmitsServiceDeactivated() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        vm.expectEmit(true, false, false, false);
        emit PayGateRegistry.ServiceDeactivated(serviceId);

        vm.prank(provider1);
        registry.deactivate(serviceId);
    }

    function test_Deactivate_NonProviderReverts() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        vm.prank(provider2);
        vm.expectRevert(PayGateRegistry.NotProvider.selector);
        registry.deactivate(serviceId);
    }

    function test_MultipleServicesFromSameProvider() public {
        vm.startPrank(provider1);
        bytes32 id1 = registry.registerService("api-1", 1000, token, "");
        bytes32 id2 = registry.registerService("api-2", 2000, token, "");
        vm.stopPrank();

        assertTrue(id1 != id2);

        PayGateRegistry.Service memory svc1 = registry.getService(id1);
        PayGateRegistry.Service memory svc2 = registry.getService(id2);
        assertEq(svc1.pricePerRequest, 1000);
        assertEq(svc2.pricePerRequest, 2000);
    }

    function test_SameNameDifferentProviders_DifferentServiceIds() public {
        vm.prank(provider1);
        bytes32 id1 = registry.registerService("shared-name", 1000, token, "");

        vm.prank(provider2);
        bytes32 id2 = registry.registerService("shared-name", 500, token, "");

        assertTrue(id1 != id2);
    }

    function test_GetService_ReturnsCorrectData() public {
        vm.prank(provider1);
        bytes32 serviceId =
            registry.registerService("my-api", 500, token, "https://meta.example.com");

        PayGateRegistry.Service memory svc = registry.getService(serviceId);
        assertEq(svc.provider, provider1);
        assertEq(svc.pricePerRequest, 500);
        assertEq(svc.acceptedToken, token);
        assertTrue(svc.active);
        assertEq(svc.metadataUri, "https://meta.example.com");
    }

    function test_GetService_RevertsForNonExistent() public {
        bytes32 fakeId = keccak256("nonexistent");
        vm.expectRevert(PayGateRegistry.ServiceNotFound.selector);
        registry.getService(fakeId);
    }

    function test_IsActive_ReturnsCorrectState() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        assertTrue(registry.isActive(serviceId));

        vm.prank(provider1);
        registry.deactivate(serviceId);

        assertFalse(registry.isActive(serviceId));
    }

    function test_ServiceRegistered_EventEmitted() public {
        bytes32 expectedId = keccak256(abi.encodePacked(provider1, "my-api"));

        vm.expectEmit(true, true, false, true);
        emit PayGateRegistry.ServiceRegistered(expectedId, provider1, 1000);

        vm.prank(provider1);
        registry.registerService("my-api", 1000, token, "");
    }

    function test_ZeroAddressToken_Reverts() public {
        vm.prank(provider1);
        vm.expectRevert(PayGateRegistry.ZeroAddress.selector);
        registry.registerService("my-api", 1000, address(0), "");
    }

    function test_ZeroPrice_IsValid() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("free-api", 0, token, "");

        PayGateRegistry.Service memory svc = registry.getService(serviceId);
        assertEq(svc.pricePerRequest, 0);
        assertTrue(svc.active);
    }

    function test_RegisterDeactivateCheckIsActive() public {
        vm.prank(provider1);
        bytes32 serviceId = registry.registerService("my-api", 1000, token, "");

        assertTrue(registry.isActive(serviceId));

        vm.prank(provider1);
        registry.deactivate(serviceId);

        assertFalse(registry.isActive(serviceId));
    }
}
