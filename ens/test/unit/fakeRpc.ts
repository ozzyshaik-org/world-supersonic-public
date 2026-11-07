// Test double for the JSON-RPC transport: answers eth_call to the Universal Resolver from a planted record table.
// It decodes what the module sends (so tests can assert on the request) and encodes a well-formed reply.
import { custom, decodeFunctionData, encodeAbiParameters, encodeFunctionResult, parseAbi, type Hex } from "viem";

export const UR_PROXY = "0xeeeeeeee14d718c2b47d9923deab1335e144eeee";
const urAbi = parseAbi([
  "function resolveWithGateways(bytes name, bytes data, string[] gateways) view returns (bytes, address)",
]);
const innerAbi = parseAbi([
  "function addr(bytes32 node) view returns (address)",
  "function addr(bytes32 node, uint256 coinType) view returns (bytes)",
  "function text(bytes32 node, string key) view returns (string)",
]);

export interface Planted {
  /** keyed by namehash (hex, lowercase) */
  addr: Record<string, Hex>;
  text: Record<string, Record<string, string>>;
}
export interface Seen { to: string; dnsName: Hex; node: Hex; fn: string; key?: string }

export function fakeTransport(planted: Planted) {
  const seen: Seen[] = [];
  const transport = custom({
    async request({ method, params }: { method: string; params?: unknown }) {
      if (method === "eth_chainId") return "0xaa36a7";
      if (method !== "eth_call") throw new Error(`unexpected RPC method ${method}`);
      const [{ to, data }] = params as [{ to: string; data: Hex }];
      const outer = decodeFunctionData({ abi: urAbi, data });
      const [dnsName, innerData] = outer.args as [Hex, Hex, string[]];
      const inner = decodeFunctionData({ abi: innerAbi, data: innerData });
      const node = (inner.args[0] as Hex).toLowerCase() as Hex;
      let result: Hex;
      if (inner.functionName === "text") {
        const key = inner.args[1] as string;
        seen.push({ to: to.toLowerCase(), dnsName, node, fn: "text", key });
        result = encodeAbiParameters([{ type: "string" }], [planted.text[node]?.[key] ?? ""]);
      } else if (inner.args.length === 2) {
        seen.push({ to: to.toLowerCase(), dnsName, node, fn: "addr(coin)" });
        result = encodeAbiParameters([{ type: "bytes" }], [planted.addr[node] ?? "0x"]);
      } else {
        seen.push({ to: to.toLowerCase(), dnsName, node, fn: "addr" });
        result = encodeAbiParameters([{ type: "address" }], [planted.addr[node] ?? "0x0000000000000000000000000000000000000000"]);
      }
      return encodeFunctionResult({
        abi: urAbi,
        functionName: "resolveWithGateways",
        result: [result, "0x1111111111111111111111111111111111111111"],
      });
    },
  });
  return { transport, seen };
}
