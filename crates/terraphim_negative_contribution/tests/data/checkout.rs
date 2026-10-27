pub struct Order {
    pub item_count: usize,
}

pub fn checkout(order: &Order) -> String {
    if order.item_count == 0 {
        return "basket is empty".to_string();
    }

    todo!("connect the payment provider")
}
