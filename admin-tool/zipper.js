// A minimal, dependency-free ZIP writer. Uses only Node's built-in zlib
// (deflateRawSync + crc32, both built in since Node 20+) — no npm package
// needed, keeping this tool zero-dependency.
//
// Only what we need: store a folder tree into a single .zip buffer with
// DEFLATE compression, that any standard unzip tool (and our Rust
// launcher) can read.

import { readdirSync, statSync, readFileSync } from "node:fs";
import path from "node:path";
import { deflateRawSync, crc32 } from "node:zlib";

function dosDateTime(date) {
  const dosTime =
    (date.getHours() << 11) | (date.getMinutes() << 5) | (date.getSeconds() >> 1);
  const dosDate =
    (((date.getFullYear() - 1980) & 0x7f) << 9) |
    ((date.getMonth() + 1) << 5) |
    date.getDate();
  return { dosTime, dosDate };
}

function listFilesRecursive(rootDir) {
  const entries = readdirSync(rootDir, { recursive: true, withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (entry.isFile()) {
      const dir = entry.parentPath || entry.path; // Node version differences
      const abs = path.join(dir, entry.name);
      const rel = path.relative(rootDir, abs).split(path.sep).join("/");
      files.push({ abs, rel });
    }
  }
  return files;
}

/// Builds a ZIP archive (Buffer) containing every file under `rootDir`,
/// with paths relative to `rootDir` (so rootDir's immediate children —
/// e.g. config/, kubejs/ — end up at the zip root).
export function createZipFromFolder(rootDir) {
  const files = listFilesRecursive(rootDir);
  const localChunks = [];
  const centralChunks = [];
  let offset = 0;

  for (const { abs, rel } of files) {
    const data = readFileSync(abs);
    const stat = statSync(abs);
    const { dosTime, dosDate } = dosDateTime(stat.mtime);
    const nameBuf = Buffer.from(rel, "utf8");
    const crc = crc32(data) >>> 0;

    let compressed = deflateRawSync(data, { level: 6 });
    let method = 8; // deflate
    if (compressed.length >= data.length) {
      // Not worth it (tiny/incompressible file) — store raw.
      compressed = data;
      method = 0;
    }

    const localHeader = Buffer.alloc(30);
    localHeader.writeUInt32LE(0x04034b50, 0);
    localHeader.writeUInt16LE(20, 4); // version needed
    localHeader.writeUInt16LE(0x0800, 6); // flag: UTF-8 filenames
    localHeader.writeUInt16LE(method, 8);
    localHeader.writeUInt16LE(dosTime, 10);
    localHeader.writeUInt16LE(dosDate, 12);
    localHeader.writeUInt32LE(crc, 14);
    localHeader.writeUInt32LE(compressed.length, 18);
    localHeader.writeUInt32LE(data.length, 22);
    localHeader.writeUInt16LE(nameBuf.length, 26);
    localHeader.writeUInt16LE(0, 28);

    localChunks.push(localHeader, nameBuf, compressed);

    const centralHeader = Buffer.alloc(46);
    centralHeader.writeUInt32LE(0x02014b50, 0);
    centralHeader.writeUInt16LE(20, 4); // version made by
    centralHeader.writeUInt16LE(20, 6); // version needed
    centralHeader.writeUInt16LE(0x0800, 8); // flag: UTF-8
    centralHeader.writeUInt16LE(method, 10);
    centralHeader.writeUInt16LE(dosTime, 12);
    centralHeader.writeUInt16LE(dosDate, 14);
    centralHeader.writeUInt32LE(crc, 16);
    centralHeader.writeUInt32LE(compressed.length, 20);
    centralHeader.writeUInt32LE(data.length, 24);
    centralHeader.writeUInt16LE(nameBuf.length, 28);
    centralHeader.writeUInt16LE(0, 30); // extra length
    centralHeader.writeUInt16LE(0, 32); // comment length
    centralHeader.writeUInt16LE(0, 34); // disk number
    centralHeader.writeUInt16LE(0, 36); // internal attrs
    centralHeader.writeUInt32LE(0, 38); // external attrs
    centralHeader.writeUInt32LE(offset, 42); // local header offset

    centralChunks.push(centralHeader, nameBuf);

    offset += localHeader.length + nameBuf.length + compressed.length;
  }

  const centralDirStart = offset;
  const centralDirBuf = Buffer.concat(centralChunks);

  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(0, 4);
  end.writeUInt16LE(0, 6);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(centralDirBuf.length, 12);
  end.writeUInt32LE(centralDirStart, 16);
  end.writeUInt16LE(0, 20);

  return Buffer.concat([...localChunks, centralDirBuf, end]);
}
