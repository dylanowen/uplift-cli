use btleplug::api;
use btleplug::api::{
    BDAddr, Characteristic, Descriptor, PeripheralProperties, Service, ValueNotification, WriteType,
};
use btleplug::platform::PeripheralId;
use mockall::mock;

use async_trait::async_trait;
use btleplug::api::CentralEvent;
use btleplug::api::CentralState;
use btleplug::api::ScanFilter;
use btleplug::Result;
use futures::stream::Stream;
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::pin::Pin;

mock! {
    pub Central {}

    #[async_trait]
    impl api::Central for Central {
        type Peripheral = MockPeripheral;

        async fn events(&self) -> Result<Pin<Box<dyn Stream<Item = CentralEvent> + Send>>>;

        async fn start_scan(&self, filter: ScanFilter) -> Result<()>;

        async fn stop_scan(&self) -> Result<()>;

        async fn peripherals(&self) -> Result<Vec<MockPeripheral>>;

        async fn peripheral(&self, id: &PeripheralId) -> Result<MockPeripheral>;

        async fn add_peripheral(&self, address: &PeripheralId) -> Result<MockPeripheral>;

        async fn adapter_info(&self) -> Result<String>;

        async fn adapter_state(&self) -> Result<CentralState>;
    }

    impl Clone for Central {
        fn clone(&self) -> Self;
    }
}

mock! {
    pub Peripheral {}

    #[async_trait]
    impl api::Peripheral for Peripheral {
        fn id(&self) -> PeripheralId;

        fn address(&self) -> BDAddr;

        async fn properties(&self) -> Result<Option<PeripheralProperties>>;

        fn services(&self) -> BTreeSet<Service>;

        async fn is_connected(&self) -> Result<bool>;

        async fn connect(&self) -> Result<()>;

        async fn disconnect(&self) -> Result<()>;

        async fn discover_services(&self) -> Result<()>;

        async fn write(&self, characteristic: &Characteristic, data: &[u8], write_type: WriteType) -> Result<()>;

        async fn read(&self, characteristic: &Characteristic) -> Result<Vec<u8>>;

        async fn subscribe(&self, characteristic: &Characteristic) -> Result<()>;

        async fn unsubscribe(&self, characteristic: &Characteristic) -> Result<()>;

        async fn notifications(&self) -> Result<Pin<Box<dyn Stream<Item=ValueNotification> + Send>>>;

        async fn write_descriptor(&self, descriptor: &Descriptor, data: &[u8]) -> Result<()>;

        async fn read_descriptor(&self, descriptor: &Descriptor) -> Result<Vec<u8>>;
    }

    impl Clone for Peripheral {
        fn clone(&self) -> Self;
    }

    impl Debug for Peripheral {
        fn fmt<'a>(&self, f: &mut Formatter<'a>) -> fmt::Result;
    }
}
