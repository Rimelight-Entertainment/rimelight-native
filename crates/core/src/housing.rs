//! Housing endpoints: `/api/housing/pets`, `/api/housing/wishlist`, and the grocery list at
//! `/api/groceries`. The pets and groceries servers keep one JSON blob per user and return the
//! whole blob from every POST, so callers replace their copy with each response.

use ehttp::{Headers, Method, Request};
use serde::{Deserialize, Serialize};

use crate::api::{ApiClient, ApiError, json_request, percent_encode};
use crate::todo::decode_ack;

/// A pet profile. Dates are `YYYY-MM-DD` strings; the server may send empty strings for unset ones.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pet {
    pub id: String,
    pub name: String,
    pub species: String,
    pub breed: Option<String>,
    pub date_of_birth: Option<String>,
    pub adoption_date: Option<String>,
    pub weight: Option<f64>,
    pub weight_unit: Option<String>,
    pub microchip_id: Option<String>,
    #[serde(default)]
    pub history: Vec<PetHistoryEntry>,
    #[serde(default)]
    pub weight_history: Vec<PetWeightEntry>,
}

/// A care log: `vet`, `vaccine`, `medication`, or `other`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetHistoryEntry {
    pub id: String,
    pub date: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub notes: Option<String>,
}

/// A weigh-in. The server keeps the newest entry first.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetWeightEntry {
    pub id: String,
    pub date: String,
    pub weight: f64,
    #[serde(default)]
    pub unit: String,
    pub notes: Option<String>,
}

/// Pet fields sent by `addPet` and `updatePet`. Empty strings are sent as empty strings, as the
/// Solid client does; `weight` is omitted when unset so the server keeps the stored value.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetDraft<'a> {
    pub name: &'a str,
    pub species: &'a str,
    pub breed: &'a str,
    pub date_of_birth: &'a str,
    pub adoption_date: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    pub weight_unit: &'a str,
    pub microchip_id: &'a str,
}

/// Body of a care log for `addHistoryEntry`.
#[derive(Debug, Serialize)]
pub struct PetHistoryDraft<'a> {
    pub date: &'a str,
    #[serde(rename = "type")]
    pub kind: &'a str,
    pub title: &'a str,
    pub notes: &'a str,
}

/// Body of a weigh-in for `addWeightEntry`.
#[derive(Debug, Serialize)]
pub struct PetWeightDraft<'a> {
    pub date: &'a str,
    pub weight: f64,
    pub unit: &'a str,
    pub notes: &'a str,
}

/// Body for `POST /api/housing/pets`. The `action` tag selects the server-side mutation.
#[derive(Debug, Serialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PetAction<'a> {
    AddPet {
        pet: &'a PetDraft<'a>,
    },
    UpdatePet {
        pet_id: &'a str,
        pet: &'a PetDraft<'a>,
    },
    DeletePet {
        pet_id: &'a str,
    },
    AddHistoryEntry {
        pet_id: &'a str,
        entry: &'a PetHistoryDraft<'a>,
    },
    DeleteHistoryEntry {
        pet_id: &'a str,
        entry_id: &'a str,
    },
    AddWeightEntry {
        pet_id: &'a str,
        entry: &'a PetWeightDraft<'a>,
    },
    DeleteWeightEntry {
        pet_id: &'a str,
        entry_id: &'a str,
    },
}

/// A wishlist item. `priority` is `low`, `medium`, or `high`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistItem {
    pub id: String,
    pub name: String,
    pub price: Option<String>,
    pub url: Option<String>,
    pub priority: String,
    pub category: String,
    pub is_purchased: bool,
}

/// Body for `POST /api/housing/wishlist`. Unset price and url go out as `null`, as in Solid.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistDraft<'a> {
    pub name: &'a str,
    pub price: Option<&'a str>,
    pub url: Option<&'a str>,
    pub priority: &'a str,
    pub category: &'a str,
}

/// Body for `PATCH /api/housing/wishlist`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WishlistPatch<'a> {
    id: &'a str,
    is_purchased: bool,
}

/// Grocery store, with an optional colour name or hex value from the Solid picker.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GroceryStore {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GroceryCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub items: Vec<GroceryItem>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroceryItem {
    pub id: String,
    pub name: String,
    pub last_price: Option<f64>,
    pub amount: f64,
    pub unit: Option<String>,
    pub brand: Option<String>,
    pub store_id: Option<String>,
    pub is_bought: bool,
    pub stock_status: StockStatus,
}

/// The stock level of a grocery item. The wire values are `in-stock`, `low`, and `out-of-stock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StockStatus {
    InStock,
    Low,
    OutOfStock,
}

/// The whole grocery list for one user, as returned by every `/api/groceries` call.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct GroceriesData {
    #[serde(default)]
    pub stores: Vec<GroceryStore>,
    #[serde(default)]
    pub categories: Vec<GroceryCategory>,
}

/// Item fields sent by `addItem` and `updateItem`. Unset last price and store are omitted, so the
/// server keeps the stored value on update, as the Solid client does.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroceryItemDraft<'a> {
    pub name: &'a str,
    pub amount: f64,
    pub unit: &'a str,
    pub brand: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_id: Option<&'a str>,
    pub stock_status: StockStatus,
    pub is_bought: bool,
}

/// Body for `POST /api/groceries`. The `action` tag selects the server-side mutation.
///
/// `ResetAllToNeedBuy` and `ClearBought` are sent as the Solid client sends them. The server
/// handler ignores them and returns the unchanged list.
#[derive(Debug, Serialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GroceryAction<'a> {
    AddCategory {
        name: &'a str,
    },
    DeleteCategory {
        cat_id: &'a str,
    },
    AddItem {
        cat_id: &'a str,
        item: &'a GroceryItemDraft<'a>,
    },
    UpdateItem {
        cat_id: &'a str,
        item_id: &'a str,
        item: &'a GroceryItemDraft<'a>,
    },
    DeleteItem {
        cat_id: &'a str,
        item_id: &'a str,
    },
    ToggleBought {
        cat_id: &'a str,
        item_id: &'a str,
    },
    SetStockStatus {
        cat_id: &'a str,
        item_id: &'a str,
        status: StockStatus,
    },
    AddStore {
        name: &'a str,
        color: &'a str,
    },
    DeleteStore {
        store_id: &'a str,
    },
    ResetAllToNeedBuy,
    ClearBought,
}

#[derive(Deserialize)]
struct PetList {
    #[serde(default)]
    pets: Vec<Pet>,
}

#[derive(Deserialize)]
struct WishlistList {
    #[serde(default)]
    wishlist: Vec<WishlistItem>,
}

pub fn decode_pets(bytes: &[u8]) -> Result<Vec<Pet>, ApiError> {
    serde_json::from_slice::<PetList>(bytes)
        .map(|list| list.pets)
        .map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_wishlist(bytes: &[u8]) -> Result<Vec<WishlistItem>, ApiError> {
    serde_json::from_slice::<WishlistList>(bytes)
        .map(|list| list.wishlist)
        .map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_wishlist_item(bytes: &[u8]) -> Result<WishlistItem, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_groceries(bytes: &[u8]) -> Result<GroceriesData, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

fn to_json(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("request bodies are plain structs and always serialize")
}

impl ApiClient {
    /// `GET /api/housing/pets`.
    pub fn pets(&self, on_done: impl FnOnce(Result<Vec<Pet>, ApiError>) + Send + 'static) {
        self.send(
            Request::get(self.url("/housing/pets")),
            decode_pets,
            on_done,
        );
    }

    /// `POST /api/housing/pets`. Returns the full pet list after the change.
    pub fn pet_action(
        &self,
        action: &PetAction<'_>,
        on_done: impl FnOnce(Result<Vec<Pet>, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/housing/pets"), to_json(action)),
            decode_pets,
            on_done,
        );
    }

    /// `GET /api/housing/wishlist`.
    pub fn wishlist(
        &self,
        on_done: impl FnOnce(Result<Vec<WishlistItem>, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url("/housing/wishlist")),
            decode_wishlist,
            on_done,
        );
    }

    /// `POST /api/housing/wishlist`. Returns the created item.
    pub fn create_wishlist_item(
        &self,
        item: &WishlistDraft<'_>,
        on_done: impl FnOnce(Result<WishlistItem, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/housing/wishlist"), to_json(item)),
            decode_wishlist_item,
            on_done,
        );
    }

    /// `PATCH /api/housing/wishlist` with `{id, isPurchased}`.
    pub fn set_wishlist_purchased(
        &self,
        id: &str,
        is_purchased: bool,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = to_json(&WishlistPatch { id, is_purchased });
        self.send(
            json_request(Method::PATCH, self.url("/housing/wishlist"), body),
            decode_ack,
            on_done,
        );
    }

    /// `DELETE /api/housing/wishlist?id=`.
    pub fn delete_wishlist_item(
        &self,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let url = format!(
            "{}?id={}",
            self.url("/housing/wishlist"),
            percent_encode(id)
        );
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            decode_ack,
            on_done,
        );
    }

    /// `GET /api/groceries`.
    pub fn groceries(
        &self,
        on_done: impl FnOnce(Result<GroceriesData, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url("/groceries")),
            decode_groceries,
            on_done,
        );
    }

    /// `POST /api/groceries`. Returns the full list after the change.
    pub fn grocery_action(
        &self,
        action: &GroceryAction<'_>,
        on_done: impl FnOnce(Result<GroceriesData, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::POST, self.url("/groceries"), to_json(action)),
            decode_groceries,
            on_done,
        );
    }
}
