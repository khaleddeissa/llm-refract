"""Provision then deactivate a synthetic OIDC subject through a local SCIM service."""

import os

from refract.client import RefractClient

client = RefractClient(
    os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:8000"),
    api_key=os.environ.get("REFRACT_API_KEY"),
)
# Use a real issuer sub only when intentionally provisioning a real identity.
user = client.request(
    "/scim/v2/Users",
    {"userName": "example@example.invalid", "externalId": "example-subject", "active": True},
)
print("Created synthetic user:", user["id"])
client.request(
    "/scim/v2/Users/" + user["id"],
    {
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{"op": "replace", "path": "active", "value": False}],
    },
    method="PATCH",
)
print("Synthetic user deactivated; authentication is now denied.")
