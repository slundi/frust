const KIND_GROUP:   u8 = 1;
const KIND_FOOD:    u8 = 2;
const KIND_RECIPE:  u8 = 3;
pub struct Transtlation {
    pub id: u32,
    pub code: String,
    pub lang: String,
    pub kind: u8,
    pub value: String
}

/// Food groups
pub struct Group {
    pub id: u16,
    pub code: String,
    pub edible: bool,
    pub frozen: bool,
    pub cool: bool,
    pub translation: String
}

const SEASON_SPRING: u8 = 0b0001;
const SEASON_SUMMER: u8 = 0b0010;
const SEASON_AUTUMN: u8 = 0b0100;
const SEASON_WINTER: u8 = 0b1000;
const NUTRITION_GRADE_A: u8 = 1;
const NUTRITION_GRADE_B: u8 = 2;
const NUTRITION_GRADE_C: u8 = 3;
const NUTRITION_GRADE_D: u8 = 4;
const NUTRITION_GRADE_E: u8 = 5;
/// Raw unit, examples: 1 banana, 2 tomatoes, ...
pub struct Food {
    pub id: u32,
    pub code: String,
    pub seasons: u8,
    /// U for "raw quantity", examples: 1 banana, 2 tomatoes
    /// G for kg, grams, ...
    /// L for liquid
    /// P for a pinch (of salt for example)
    /// S for a tablespoon
    /// s for a teaspoon
    /// Z for zest (of lemon for example)
    pub unit: char,
    /// Nutri-score A-E
    pub nutrition_grade_fr: u8,
    // energy values 1 cal = 4.184 joules
    // energy values per 100g/100ml
    /// energy in kcal
    pub energy: u16,
    pub fat: u16,
    pub satured_fat: us16,
    pub carbohydrates: u16,
    pub sugars: u16,
    pub fiber: u16,
    pub protein: u16,
    pub salt: u16,
}

pub struct Recipe_Step {
    pub id: u32,
    pub instructions: String,
    //ingredient, quantity
}

pub struct Recipe {
    pub id: u32,
    pub title: String,
    /// Save recipe as markdown
    pub instructions: String,
}

const DAY_MONDAY:    u8 = 0b0000001;
const DAY_TUESDAY:   u8 = 0b0000010;
const DAY_WEDNESDAY: u8 = 0b0000100;
const DAY_THURSDAY:  u8 = 0b0001000;
const DAY_FRIDAY:    u8 = 0b0010001;
const DAY_SATURDAY:  u8 = 0b0100000;
const DAY_SUNDAY:    u8 = 0b1000000;
const TIME_BREAKFAST: u8 = 0b0001;
const TIME_LUNCH:     u8 = 0b0010;
const TIME_SNACK:     u8 = 0b0100;
const TIME_DINNER:    u8 = 0b1000;
pub struct Dish {
    pub id: u32,
    pub day: u8,
    pub time: u8
}

pub struct Grocery_List {
    pub id: u32,
    /// Date format: YYMMDD
    pub date: u32,
    pub store: String,
    pub completed: bool
}

pub struct Grocery_Line {
    pub id: u32,
    pub list_id: u32,
    pub food_id: u32,
    pub ean: Option<u64>,
    pub price: Option<f32>
}

pub struct User {
    pub id: u32,
    pub login: String,
    pub password: String,
    pub email: String,
    /// Currency symbol
    pub currency: char
}

pub struct Favorite_Food {
    pub user_id: u32,
    pub food_id: u32
}

pub struct Favorite_Dish {
    pub user_id: u32,
    pub dish_id: u32,
    /// how many persons can eat the dish
    pub serving: u8
}

pub struct Meal {
    pub user_id: u32,
    pub dish_id: u32,
    pub day: u8,
}

const KIND_VILLAGE_MARKET: u8 = 1;
const KIND_SUPERMARKET: u8 = 2;
const KIND_SUPERMARKET_WITH_DRIVE: u8 = 4;
const KIND_DRIVE: u8 = 8;
const KIND_INTERNET: u8 = 16;
pub struct Store {
    pub id: u32,
    pub name: String,
    pub kind: u8,
    /// 3-digits country code [Wikipedia](https://en.wikipedia.org/wiki/ISO_3166-1_numeric)
    pub country: u16,
    pub city: String,
    // float(10,6) is enough
    pub latitude: f32, pub longitude: f32,
    //chain/franchise?
    //icon?
}

/// User's favorite stores
pub struct Favorite_Store {
    pub user_id: u32,
    pub store_id: u32,
}
