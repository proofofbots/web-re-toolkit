import { WreError, ErrorKind } from "./errors.js";

const HEADER_BYTES = 8;
const MAX_JSON_BYTES = 64 * 1024 * 1024;
const MAX_BIN_BYTES = 512 * 1024 * 1024;

export function encodeFrame(jsonValue, bin) {
  const json = Buffer.from(JSON.stringify(jsonValue), "utf8");
  const binBuf = bin === undefined || bin === null ? Buffer.alloc(0) : Buffer.from(bin);
  const header = Buffer.alloc(HEADER_BYTES);
  header.writeUInt32BE(json.length, 0);
  header.writeUInt32BE(binBuf.length, 4);
  return Buffer.concat([header, json, binBuf]);
}

export class FrameDecoder {
  constructor() {
    this.buffer = Buffer.alloc(0);
  }

  push(chunk) {
    const incoming = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    this.buffer = this.buffer.length === 0 ? incoming : Buffer.concat([this.buffer, incoming]);

    const frames = [];
    for (;;) {
      if (this.buffer.length < HEADER_BYTES) break;
      const jsonLen = this.buffer.readUInt32BE(0);
      const binLen = this.buffer.readUInt32BE(4);
      if (jsonLen > MAX_JSON_BYTES) {
        throw new WreError(ErrorKind.Protocol, `frame json length ${jsonLen} exceeds the 64 MiB cap`);
      }
      if (binLen > MAX_BIN_BYTES) {
        throw new WreError(ErrorKind.Protocol, `frame binary length ${binLen} exceeds the 512 MiB cap`);
      }
      const total = HEADER_BYTES + jsonLen + binLen;
      if (this.buffer.length < total) break;

      const jsonBytes = this.buffer.subarray(HEADER_BYTES, HEADER_BYTES + jsonLen);
      const binBytes = this.buffer.subarray(HEADER_BYTES + jsonLen, total);
      const json = JSON.parse(jsonBytes.toString("utf8"));
      const bin = binLen === 0 ? Buffer.alloc(0) : Buffer.from(binBytes);
      frames.push({ json, bin });

      this.buffer = this.buffer.subarray(total);
    }
    return frames;
  }
}
