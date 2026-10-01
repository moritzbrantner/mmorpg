//! Immutable item content and bounded bag rules. Player ownership and queued
//! intents are integrated separately; this value never grants network authority.

use std::{error::Error, fmt};

pub const INVENTORY_SLOTS: usize = 16;
pub const ITEM_CATALOG_REVISION: u64 = 1;

/// Stable catalog identity. Zero and unknown values fail closed on lookup.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ItemId(u16);

impl ItemId {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemTemplate {
    pub id: ItemId,
    pub name: &'static str,
    pub max_stack: u16,
}

/// Minimal starter catalog, in stable ID order. These are bag items only;
/// equipment, consumable effects, prices and loot tables are separate rules.
pub const ITEM_CATALOG: [ItemTemplate; 2] = [
    ItemTemplate {
        id: ItemId::new(1),
        name: "Torn Fur",
        max_stack: 20,
    },
    ItemTemplate {
        id: ItemId::new(2),
        name: "Worn Dagger",
        max_stack: 1,
    },
];

#[must_use]
pub fn item_template(id: ItemId) -> Option<&'static ItemTemplate> {
    ITEM_CATALOG.iter().find(|item| item.id == id)
}

/// A nonempty, catalog-valid stack. Private fields prevent invalid imported
/// state; decoders must use `new` rather than repairing an invalid quantity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemStack {
    item: ItemId,
    quantity: u16,
}

impl ItemStack {
    pub fn new(item: ItemId, quantity: u16) -> Result<Self, InventoryError> {
        let template = item_template(item).ok_or(InventoryError::UnknownItem)?;
        if quantity == 0 || quantity > template.max_stack {
            return Err(InventoryError::InvalidQuantity);
        }
        Ok(Self { item, quantity })
    }

    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    #[must_use]
    pub const fn quantity(self) -> u16 {
        self.quantity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InventoryError {
    UnknownItem,
    InvalidQuantity,
    InvalidSlot,
    EmptySlot,
    InsufficientItems,
    NoCapacity,
    IncompatibleStacks,
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownItem => "unknown item",
            Self::InvalidQuantity => "invalid item quantity",
            Self::InvalidSlot => "invalid inventory slot",
            Self::EmptySlot => "empty inventory slot",
            Self::InsufficientItems => "insufficient items",
            Self::NoCapacity => "insufficient inventory capacity",
            Self::IncompatibleStacks => "cannot split into a different item stack",
        })
    }
}

impl Error for InventoryError {}

/// Exactly 16 ordered slots. Mutations are atomic and allocation-free.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inventory {
    slots: [Option<ItemStack>; INVENTORY_SLOTS],
}

impl Inventory {
    /// Imported stacks have already passed `ItemStack::new`; array length
    /// enforces capacity without accepting or truncating excess slots.
    #[must_use]
    pub const fn from_slots(slots: [Option<ItemStack>; INVENTORY_SLOTS]) -> Self {
        Self { slots }
    }

    #[must_use]
    pub const fn slots(&self) -> &[Option<ItemStack>; INVENTORY_SLOTS] {
        &self.slots
    }

    /// Fill existing matching stacks in slot order, then empty slots in slot
    /// order. Either the entire quantity fits or the original bag is retained.
    pub fn insert(&mut self, item: ItemId, quantity: u16) -> Result<(), InventoryError> {
        let template = item_template(item).ok_or(InventoryError::UnknownItem)?;
        if quantity == 0 {
            return Err(InventoryError::InvalidQuantity);
        }
        let mut slots = self.slots;
        let mut remaining = quantity;
        for stack in slots
            .iter_mut()
            .flatten()
            .filter(|stack| stack.item == item)
        {
            let added = remaining.min(template.max_stack - stack.quantity);
            stack.quantity += added;
            remaining -= added;
        }
        for slot in &mut slots {
            if slot.is_none() && remaining > 0 {
                let added = remaining.min(template.max_stack);
                *slot = Some(ItemStack {
                    item,
                    quantity: added,
                });
                remaining -= added;
            }
        }
        if remaining > 0 {
            return Err(InventoryError::NoCapacity);
        }
        self.slots = slots;
        Ok(())
    }

    /// Move/split into an empty slot or merge with the same item. A complete
    /// stack moved onto a different item swaps slots; a partial swap is refused.
    /// A validated move to the same slot is a no-op. No partial merges occur.
    pub fn move_stack(
        &mut self,
        source: usize,
        destination: usize,
        quantity: u16,
    ) -> Result<(), InventoryError> {
        let source_stack = self.slots.get(source).ok_or(InventoryError::InvalidSlot)?;
        let destination_stack = self
            .slots
            .get(destination)
            .ok_or(InventoryError::InvalidSlot)?;
        let stack = source_stack.ok_or(InventoryError::EmptySlot)?;
        if quantity == 0 {
            return Err(InventoryError::InvalidQuantity);
        }
        if quantity > stack.quantity {
            return Err(InventoryError::InsufficientItems);
        }
        if source == destination {
            return Ok(());
        }
        let target = match *destination_stack {
            Some(target) if target.item != stack.item => {
                if quantity != stack.quantity {
                    return Err(InventoryError::IncompatibleStacks);
                }
                self.slots.swap(source, destination);
                return Ok(());
            }
            Some(target) => {
                let max_stack = item_template(stack.item)
                    .ok_or(InventoryError::UnknownItem)?
                    .max_stack;
                if quantity > max_stack - target.quantity {
                    return Err(InventoryError::NoCapacity);
                }
                ItemStack {
                    item: stack.item,
                    quantity: target.quantity + quantity,
                }
            }
            None => ItemStack {
                item: stack.item,
                quantity,
            },
        };
        self.slots[destination] = Some(target);
        self.slots[source] = if quantity == stack.quantity {
            None
        } else {
            Some(ItemStack {
                item: stack.item,
                quantity: stack.quantity - quantity,
            })
        };
        Ok(())
    }
}
