# Open DB Viewer

Open DB Viewer is a premium, mac-first, secure desktop database client built using **Tauri**, **Rust**, and **Svelte 5**. It supports relational databases (PostgreSQL) and key-value stores (Redis) with a visual, responsive interface.

---

## 1. Installation Process (macOS)

### Prerequisites
Make sure you have Rust and Node.js installed on your Mac.

1. **Install Rust (via rustup):**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Install Node.js (v18+ recommended):**
   Available via Homebrew:
   ```bash
   brew install node
   ```

### Setup & Run
1. **Clone the repository and enter the directory:**
   ```bash
   git clone <repository-url>
   ```
2. **Install frontend dependencies:**
   ```bash
   npm install
   ```
3. **Run the application in development mode:**
   ```bash
   npm run tauri dev
   ```
4. **Build a production standalone DMG/App bundle:**
   ```bash
   npm run tauri build
   ```

---

## 2. Keychain Security Architecture

To keep database credentials secure, Open DB Viewer integrates with the native **macOS Keychain Services API**:

- **No Plaintext Passwords:** Saved profiles are stored in `connections.json` under the user's config directory (e.g., `~/Library/Application Support/com.santiagomusso.tauri-app/connections.json`), but their passwords are encrypted and stored inside the system Keychain.
- **Keyring Integration:** The Rust backend uses the `keyring` crate to bind connections.
  - **Keychain Service Identifier:** `com.santiagomusso.tauri-app`
  - **Account Identifier:** The unique connection profile UUID.
- **Connection Handshake:** When initiating a session, the app fetches the password from the system Keychain on demand, establishes the driver pool, and discards the password from memory.

---

## 3. Redis Commands Capability

The Redis Key-Value Explorer driver provides high-performance introspective commands directly through Tauri IPC handlers:

- **Key Scanning (`SCAN`):** Performs non-blocking keys scanning with customizable cursor offsets and pattern matching (e.g., `users:*`), ensuring the server remains responsive.
- **Type Resolution (`TYPE`):** Automatically maps keys to their underlying structures (`String`, `List`, `Set`, `Hash`, `ZSet`).
- **Value Mutation:** Supports reading, updating, and inserting values:
  - **Strings:** Direct raw text reading/writing.
  - **Lists:** `LRANGE` list collections.
  - **Sets:** `SMEMBERS` unique member listings.
  - **Hashes:** `HGETALL` fields and values dictionary editing.
  - **Sorted Sets (ZSet):** `ZRANGE` scored collections.
- **TTL Operations:** View, modify, or persist key Time-To-Live (TTL) attributes natively.
- **Diagnostics (`INFO`):** Generates stats grids for memory allocation, client loads, uptime, and detailed server internals.

---

## 4. Entity-Relationship (ER) Layout Engine

The PostgreSQL driver provides interactive database visualizations:

- **Database Introspection:** Queries `information_schema.table_constraints` and `information_schema.key_column_usage` to extract relationships, table column fields, primary keys, and foreign keys.
- **Diagram Canvas (`@xyflow/svelte`):** Renders interactive nodes representing tables, with columns mapping to incoming/outgoing relation connector handles on the node sides.
- **Automated Layouts (`dagre`):** Applies Directed Acyclic Graph layout parameters programmatically on the backend coordinates, minimizing intersecting link paths.
- **DDL Inspector:** Double-clicking a table node opens a slide-out panel that queries the backend to output structural SQL DDL.
