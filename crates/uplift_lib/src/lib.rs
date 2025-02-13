mod api;
mod desk;
mod id;
#[cfg(test)]
mod mock_btle;

pub use crate::api::*;
pub use crate::desk::*;
pub use crate::id::*;

// // we shouldn't need greater precision than millimeter
// pub type SI<V> = dyn Units<
//     V,
//     length = millimeter,
//     mass = kilogram,
//     time = second,
//     electric_current = ampere,
//     thermodynamic_temperature = kelvin,
//     amount_of_substance = mole,
//     luminous_intensity = candela,
// >;
// pub type Length = si::length::Length<SI<f32>, f32>;
