//! Native outlined fields use the content surface, independently of border color.

mod placeholder;
pub use placeholder::placeholder;

use crate::app::AppExt as _;
use gpui_kit::component::{
    input::{Input, InputState},
    select::{Select, SelectDelegate, SelectItem, SelectState},
};
use gpui_kit::{App, Entity, Styled};

pub fn input(state: &Entity<InputState>, cx: &App) -> Input {
    Input::new(state).bg(cx.gds().appearance().theme().colors().content)
}

pub fn select<D>(state: &Entity<SelectState<D>>, cx: &App) -> Select<D>
where
    D: SelectDelegate + 'static,
    <D::Item as SelectItem>::Value: PartialEq + Clone,
{
    Select::new(state).bg(cx.gds().appearance().theme().colors().content)
}
