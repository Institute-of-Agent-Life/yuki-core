"""Python-level loopback-only boundary; does not claim native OS isolation."""

import ipaddress
import socket


def install():
    def allowed(address):
        if isinstance(address, (str, bytes)):
            return  # Unix-domain socket path.
        host = address[0]
        if host == "localhost":
            return
        try:
            if ipaddress.ip_address(host).is_loopback:
                return
        except ValueError:
            pass
        raise PermissionError("offline case: non-loopback network denied")

    for name in ("connect", "connect_ex", "bind"):
        original = getattr(socket.socket, name)

        def guarded(self, address, _original=original):
            allowed(address)
            return _original(self, address)

        setattr(socket.socket, name, guarded)

    original_sendto = socket.socket.sendto

    def sendto(self, data, *args):
        allowed(args[-1])
        return original_sendto(self, data, *args)

    socket.socket.sendto = sendto
    original_getaddrinfo = socket.getaddrinfo

    def getaddrinfo(host, port, *args, **kwargs):
        allowed((host, port))
        result = original_getaddrinfo(host, port, *args, **kwargs)
        for item in result:
            allowed(item[4])
        return result

    socket.getaddrinfo = getaddrinfo

    def dns(host):
        allowed((host, 0))
        return "127.0.0.1" if host == "localhost" else host

    socket.gethostbyname = dns
    socket.gethostbyname_ex = lambda host: (host, [], [dns(host)])
