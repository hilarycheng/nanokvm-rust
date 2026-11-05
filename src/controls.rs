//! Standard image controls, read and written only by the capture worker.
use anyhow::{bail, ensure, Context, Result};
use v4l::{control::{Control, Description, Flags, Type, Value}, Device};

#[derive(Debug)]
pub enum Request { Refresh, Set { id: u32, value: i64 } }

#[derive(Debug)]
pub struct ImageControl {
    pub description: Description,
    pub value: Option<i64>,
    pub unavailable: Option<String>,
}

pub struct Update {
    pub controls: Result<Vec<ImageControl>, String>,
    pub message: String,
}

pub trait ControlDevice {
    fn descriptions(&self) -> Result<Vec<Description>>;
    fn read(&self, id: u32) -> Result<Value>;
    fn write(&self, control: Control) -> Result<()>;
}

impl ControlDevice for Device {
    fn descriptions(&self) -> Result<Vec<Description>> { self.query_controls().context("Enumerate V4L2 controls") }
    fn read(&self, id: u32) -> Result<Value> { Ok(self.control(id)?.value) }
    fn write(&self, control: Control) -> Result<()> { self.set_control(control).context("Write V4L2 control") }
}

fn image_control(id: u32) -> bool {
    // Linux standard brightness/color, gain, exposure, white balance, and anti-flicker controls.
    matches!(id, 0x00980900..=0x00980903 | 0x0098090c | 0x0098090e..=0x00980913 |
        0x00980918..=0x0098091c | 0x009a0901..=0x009a0903 | 0x009a0919)
}

fn availability(desc: &Description) -> Result<()> {
    ensure!(matches!(desc.typ, Type::Integer | Type::Boolean | Type::Menu), "Unsupported control type: {}", desc.typ);
    ensure!(!desc.flags.intersects(Flags::DISABLED | Flags::READ_ONLY | Flags::INACTIVE | Flags::GRABBED | Flags::WRITE_ONLY | Flags::HAS_PAYLOAD),
        "Unavailable: {}", desc.flags);
    ensure!(desc.minimum >= i32::MIN as i64 && desc.maximum <= i32::MAX as i64
        && desc.minimum <= desc.maximum && desc.step > 0, "Invalid control range or step");
    Ok(())
}

pub fn validate(desc: &Description, value: i64) -> Result<()> {
    availability(desc)?;
    ensure!(value >= desc.minimum && value <= desc.maximum, "Value outside control range");
    ensure!((value as i128 - desc.minimum as i128) % desc.step as i128 == 0, "Value does not match control step");
    if desc.typ == Type::Boolean { ensure!(value == 0 || value == 1, "Invalid boolean value"); }
    if desc.typ == Type::Menu {
        ensure!(desc.items.as_ref().is_some_and(|items| items.iter().any(|(id, _)| *id as i64 == value)), "Unsupported menu choice");
    }
    Ok(())
}

fn snapshot(device: &impl ControlDevice) -> Result<Vec<ImageControl>> {
    device.descriptions()?.into_iter().filter(|desc| image_control(desc.id)).map(|description| {
        let mut unavailable = availability(&description).err().map(|error| error.to_string());
        let value = if description.flags.intersects(Flags::DISABLED | Flags::WRITE_ONLY | Flags::HAS_PAYLOAD)
            || !matches!(description.typ, Type::Integer | Type::Boolean | Type::Menu) { None }
        else { match device.read(description.id) {
            Ok(Value::Integer(value)) => Some(value),
            Ok(Value::Boolean(value)) => Some(i64::from(value)),
            Ok(_) => { unavailable = Some("Unsupported control value".into()); None }
            Err(error) => { unavailable = Some(format!("Read failed: {error:#}")); None }
        } };
        if unavailable.is_none() {
            if let Some(value) = value {
                unavailable = validate(&description, value).err().map(|error| format!("Invalid device value: {error}"));
            }
        }
        Ok(ImageControl { description, value, unavailable })
    }).collect()
}

pub fn process(device: &impl ControlDevice, request: Request) -> Update {
    let result = (|| -> Result<String> {
        match request {
            Request::Refresh => Ok("Device controls refreshed".into()),
            Request::Set { id, value } => {
                ensure!(image_control(id), "Not a standard image control");
                let descriptions = device.descriptions()?;
                let Some(desc) = descriptions.iter().find(|desc| desc.id == id) else { bail!("Control is no longer available"); };
                validate(desc, value)?;
                let value = if desc.typ == Type::Boolean { Value::Boolean(value != 0) } else { Value::Integer(value) };
                device.write(Control { id, value })?;
                Ok(format!("{} applied; values read back from device", desc.name))
            }
        }
    })();
    Update {
        controls: snapshot(device).map_err(|error| format!("{error:#}")),
        message: result.unwrap_or_else(|error| format!("Device control failed: {error:#}")),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::Cell;

    pub fn gamma() -> Description {
        Description { id: 0x00980910, typ: Type::Integer, name: "Device gamma".into(),
            minimum: 1, maximum: 9, step: 2, default: 3, flags: Flags::empty(), items: None }
    }

    struct DeviceStub { value: Cell<i64>, flags: Cell<Flags>, read_error: bool, write_error: bool, missing: bool }
    impl ControlDevice for DeviceStub {
        fn descriptions(&self) -> Result<Vec<Description>> {
            let mut desc = gamma();
            desc.flags = self.flags.get();
            Ok(if self.missing { vec![] } else { vec![desc] })
        }
        fn read(&self, _: u32) -> Result<Value> {
            ensure!(!self.read_error, "Device read failed");
            Ok(Value::Integer(self.value.get()))
        }
        fn write(&self, control: Control) -> Result<()> {
            ensure!(!self.write_error, "Device write failed");
            let Value::Integer(value) = control.value else { bail!("Wrong value type"); };
            self.value.set(value);
            Ok(())
        }
    }

    #[test]
    fn validate_ranges_steps_flags_and_sparse_menus() {
        let mut desc = gamma();
        for value in [1, 3, 9] { validate(&desc, value).unwrap(); }
        for value in [i64::MIN, 0, 2, 10, i64::MAX] { assert!(validate(&desc, value).is_err()); }
        for flag in [Flags::DISABLED, Flags::READ_ONLY, Flags::INACTIVE, Flags::GRABBED, Flags::WRITE_ONLY, Flags::HAS_PAYLOAD] {
            desc.flags = flag;
            assert!(validate(&desc, 3).is_err());
        }
        desc.flags = Flags::empty();
        desc.step = 0;
        assert!(validate(&desc, 3).is_err());
        desc.step = 1;
        desc.minimum = 0;
        desc.maximum = 3;
        desc.typ = Type::Menu;
        desc.items = Some(vec![(0, v4l::control::MenuItem::Name("Auto".into())), (3, v4l::control::MenuItem::Name("Manual".into()))]);
        validate(&desc, 3).unwrap();
        assert!(validate(&desc, 1).is_err());
        desc.typ = Type::Boolean;
        validate(&desc, 1).unwrap();
        assert!(validate(&desc, 3).is_err());
        desc.typ = Type::String;
        assert!(validate(&desc, 1).is_err());
    }

    #[test]
    fn writes_requery_availability_and_return_confirmed_values_or_errors() {
        let mut device = DeviceStub { value: Cell::new(3), flags: Cell::new(Flags::empty()), read_error: false, write_error: false, missing: false };
        let set = || Request::Set { id: gamma().id, value: 7 };
        let update = process(&device, set());
        assert!(update.message.contains("applied"));
        assert_eq!(update.controls.unwrap()[0].value, Some(7));
        device.value.set(3);
        device.flags.set(Flags::INACTIVE);
        let update = process(&device, set());
        assert!(update.message.contains("Unavailable"));
        assert!(update.controls.unwrap()[0].unavailable.is_some());
        assert_eq!(device.value.get(), 3);
        device.flags.set(Flags::empty());
        device.write_error = true;
        let update = process(&device, set());
        assert!(update.message.contains("Device write failed"));
        assert_eq!(update.controls.unwrap()[0].value, Some(3));
        device.read_error = true;
        let controls = process(&device, Request::Refresh).controls.unwrap();
        assert!(controls[0].value.is_none());
        assert!(controls[0].unavailable.as_ref().unwrap().contains("Read failed"));
        device.missing = true;
        assert!(process(&device, set()).message.contains("no longer available"));
        assert!(process(&device, Request::Set { id: 0, value: 0 }).message.contains("Not a standard image control"));
    }
}
