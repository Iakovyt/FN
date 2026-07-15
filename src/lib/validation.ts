// IP / CIDR validation used before sending an ipset entry to the Rust backend.

function isValidIPv4(ip: string): boolean {
  const parts = ip.split(".");
  if (parts.length !== 4) return false;
  return parts.every((p) => {
    if (!/^\d{1,3}$/.test(p)) return false;
    const n = Number(p);
    return n >= 0 && n <= 255 && String(n) === p; // reject "01", "007"
  });
}

function isValidIPv6(ip: string): boolean {
  // Compact but reasonably strict IPv6 check (supports "::" compression).
  if (ip.indexOf(":") === -1) return false;
  const doubleColon = ip.split("::");
  if (doubleColon.length > 2) return false;

  const validGroup = (g: string) => /^[0-9a-fA-F]{1,4}$/.test(g);

  if (doubleColon.length === 2) {
    const head = doubleColon[0] ? doubleColon[0].split(":") : [];
    const tail = doubleColon[1] ? doubleColon[1].split(":") : [];
    if (head.length + tail.length > 7) return false;
    return [...head, ...tail].every(validGroup);
  }

  const groups = ip.split(":");
  if (groups.length !== 8) return false;
  return groups.every(validGroup);
}

/** Returns true for a bare IPv4/IPv6 address or a valid CIDR (a.b.c.d/nn). */
export function isValidIpOrCidr(value: string): boolean {
  const v = value.trim();
  if (!v) return false;

  const slash = v.indexOf("/");
  if (slash === -1) {
    return isValidIPv4(v) || isValidIPv6(v);
  }

  const addr = v.slice(0, slash);
  const maskRaw = v.slice(slash + 1);
  if (!/^\d{1,3}$/.test(maskRaw)) return false;
  const mask = Number(maskRaw);

  if (isValidIPv4(addr)) return mask >= 0 && mask <= 32;
  if (isValidIPv6(addr)) return mask >= 0 && mask <= 128;
  return false;
}
