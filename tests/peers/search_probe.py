"""One loopback HTTP write from the synthetic SDK consumer after a real Host permit."""

import hashlib
import http.client
import ipaddress
import json
from urllib.parse import urlsplit


def observe(permit, original):
    target = urlsplit(permit["endpoint"])
    assert permit["dispatch_now"] and permit["handle"] == original["params"]["handle"]
    assert target.scheme == "http" and ipaddress.ip_address(target.hostname).is_loopback
    connection = http.client.HTTPConnection(target.hostname, target.port, timeout=5)
    try:
        connection.request(
            "POST", target.path, json.dumps(original["params"]["arguments"])
        )
        response = connection.getresponse()
        body = response.read()
        result = {
            "outcome": "response",
            "status": response.status,
            "response_sha256": hashlib.sha256(body).hexdigest(),
        }
        if "usage" in (data := json.loads(body)):
            result["usage"] = data["usage"]
        return result
    except (OSError, http.client.HTTPException):
        return {"outcome": "unknown"}
    finally:
        connection.close()
