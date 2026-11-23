// SPDX-License-Identifier: GPL-2.0-only
/*
 * The one C sliver of the Rust as7760_fan: i2c_check_functionality() is a
 * static inline with no Rust helper in 7.3, and reaching it from Rust would
 * mean dereferencing adapter->algo->functionality by hand.
 */
#include <linux/i2c.h>

bool as7760_fan_smbus_byte_data_ok(const struct i2c_client *client);
bool as7760_fan_smbus_byte_data_ok(const struct i2c_client *client)
{
	return i2c_check_functionality(client->adapter, I2C_FUNC_SMBUS_BYTE_DATA);
}
