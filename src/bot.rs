use crate::broker::Broker;
use anyhow::Result;

pub fn run_bot(mut broker: Broker) -> Result<()> {
    // Replace this with your strategy loop.
    let h = broker.holdings()?;
    println!("Holdings: {h}");
    Ok(())
}
