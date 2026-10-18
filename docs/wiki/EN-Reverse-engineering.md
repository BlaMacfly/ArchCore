# Client reverse engineering

The protocol and data formats were rebuilt from the client executable, with Ghidra 12 driven by
[REA](https://github.com/morluto/rea). The client is an x86-64 PE executable of about 70,000
functions, without symbols but with RTTI for its C++ classes (`AgcmLogin`, `SoundManager`…).

## Method

1. Find a telling string ("LG_LoginMainCreateButtonClick", "Sound_Option"…).
2. Go back to the function that uses it, then decompile its calls.
3. For packets: decompile the `Agpm*` module constructors, which declare every field with
   `AuPacket::SetFieldType`. `tools/re/extract-schemas.mjs` turns them into `data/re/packet-schemas.json`.
4. Check each hypothesis with the simulator, then with the real client and the `ARCHCORE_TRACE=1` log.

## Main findings

| Topic | Finding |
|---|---|
| Server address | The server list file is RC4-encrypted with key MD5("1111") through CryptoAPI; its plain `…Test.ini` variant is read first |
| `Game/ini` tables | Same encryption; CP949 text. A second key, "asdfqwer", is used by another loader |
| Packet header | 19 bytes, u32 lengths (u16 in the 32-bit client) |
| Encryption | Blowfish ECB, little-endian words; `A1`/`B1` … `AF`/`BF` frames |
| Password | 65 bytes, RC4-encrypted with an 8-character key sent by the server |
| Type 3 | 4-byte field flags, set through a setter rather than a direct write |
| Character creation | The "Create" button reads the race received with login op 3; it only acts for race 1 or 2 |
| Language | `Launcher/LanguageInfo.txt`: 17 = French (`ini_fr`), 5 = English |
| Sound | Modern FMOD, 200 voices; 2D sounds (types 1 and 2) or 3D with linear rolloff (types 3 and 4) |
| Updates | The game patches itself: function addresses change between versions, schemas do not |

## Reference

The open source ALEmu project, written for the 32-bit client, served as a comparison point.
ArchCore does not reuse its code.
