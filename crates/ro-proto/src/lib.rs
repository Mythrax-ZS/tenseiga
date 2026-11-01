//! rAthena wire protocol: framing + packet definitions. No I/O.
//!
//! Target: PACKETVER 20250402 (main). Layouts taken from rathena
//! src/common/packets.hpp, src/map/packets.hpp and src/map/clif_shuffle.hpp.

pub mod bg;
pub mod chatroom;
pub mod codec;
pub mod companions;
pub mod generated;
pub mod gm;
pub mod guild;
pub mod items;
pub mod mail;
pub mod market;
pub mod menus;
pub mod merchant;
pub mod quests;
pub mod refine;
pub mod search;
pub mod packets;
pub mod progress;
pub mod social;
pub mod table;

pub use codec::{CodecError, Encode, Frame, RoCodec};
pub use generated::{id, PACKETVER};
pub use table::{client_packet, server_name, ClientPacket, PacketLen, PacketTable, ServerTable};
