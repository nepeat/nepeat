// SPDX-License-Identifier: GPL-2.0-only
/* AS7760 fan CPLD, independently implemented from captured cpldutil behavior.
 * I2C2 -> PCA9548 0x76 channel 0 -> 0x66. See ../../thermal.md.
 * Probe only reads. Duty writes follow the vendor's 0x33=0, 0x11=duty sequence.
 */
#include <linux/hwmon.h>
#include <linux/i2c.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/of.h>

struct as7760_fan { struct i2c_client *client; struct mutex lock; };
static const char * const labels[] = {
	"fan1_front", "fan1_rear", "fan2_front", "fan2_rear",
	"fan3_front", "fan3_rear", "fan4_front", "fan4_rear",
	"fan5_front", "fan5_rear", "fan6_front", "fan6_rear",
};

static umode_t fan_visible(const void *data, enum hwmon_sensor_types type,
			 u32 attr, int channel)
{
	if (type == hwmon_pwm && attr == hwmon_pwm_input) return 0644;
	if (type == hwmon_fan) return 0444;
	return 0;
}

static int fan_read(struct device *dev, enum hwmon_sensor_types type,
		    u32 attr, int channel, long *val)
{
	struct as7760_fan *fan = dev_get_drvdata(dev);
	int raw, present, ret = 0;
	mutex_lock(&fan->lock);
	if (type == hwmon_pwm && attr == hwmon_pwm_input) {
		raw = i2c_smbus_read_byte_data(fan->client, 0x11);
		if (raw < 0) ret = raw;
		else if (raw > 15) ret = -EIO;
		else *val = raw * 17;
	} else if (type == hwmon_fan && channel < ARRAY_SIZE(labels)) {
		raw = i2c_smbus_read_byte_data(fan->client,
			(channel % 2 ? 0x22 : 0x12) + channel / 2);
		if (raw < 0) { ret = raw; goto out; }
		if (attr == hwmon_fan_input) *val = raw * 100;
		else if (attr == hwmon_fan_fault) {
			present = i2c_smbus_read_byte_data(fan->client, 0x0f);
			if (present < 0) ret = present;
			else *val = !!(present & BIT(channel / 2)) || !raw;
		} else ret = -EOPNOTSUPP;
	} else ret = -EOPNOTSUPP;
out:
	mutex_unlock(&fan->lock);
	return ret;
}

static int fan_read_string(struct device *dev, enum hwmon_sensor_types type,
			   u32 attr, int channel, const char **str)
{
	if (type != hwmon_fan || attr != hwmon_fan_label || channel >= ARRAY_SIZE(labels))
		return -EOPNOTSUPP;
	*str = labels[channel];
	return 0;
}

static int fan_write(struct device *dev, enum hwmon_sensor_types type,
		     u32 attr, int channel, long val)
{
	struct as7760_fan *fan = dev_get_drvdata(dev);
	int ret, check, duty;
	if (type != hwmon_pwm || attr != hwmon_pwm_input || channel != 0)
		return -EOPNOTSUPP;
	/* Bring-up guard: no fan-off or very low duty requests. Round upwards. */
	if (val < 128 || val > 255) return -EINVAL;
	duty = DIV_ROUND_UP(val * 15, 255);
	mutex_lock(&fan->lock);
	ret = i2c_smbus_write_byte_data(fan->client, 0x33, 0);
	if (ret) goto out;
	ret = i2c_smbus_write_byte_data(fan->client, 0x11, duty);
	if (!ret) {
		check = i2c_smbus_read_byte_data(fan->client, 0x11);
		if (check != duty) ret = check < 0 ? check : -EIO;
	}
	/* If a requested duty cannot be verified, attempt full speed and report
	 * failure. A broken bus cannot provide a software cooling guarantee. */
	if (ret && i2c_smbus_write_byte_data(fan->client, 0x11, 15))
		dev_err(dev, "fan duty failed and full-speed fallback failed\n");
out:
	mutex_unlock(&fan->lock);
	return ret;
}

static const struct hwmon_ops fan_ops = {
	.is_visible = fan_visible, .read = fan_read,
	.read_string = fan_read_string, .write = fan_write,
};
#define FAN_ATTRS (HWMON_F_INPUT | HWMON_F_FAULT | HWMON_F_LABEL)
static const struct hwmon_channel_info * const fan_info[] = {
	HWMON_CHANNEL_INFO(pwm, HWMON_PWM_INPUT),
	HWMON_CHANNEL_INFO(fan, FAN_ATTRS, FAN_ATTRS, FAN_ATTRS, FAN_ATTRS,
		FAN_ATTRS, FAN_ATTRS, FAN_ATTRS, FAN_ATTRS,
		FAN_ATTRS, FAN_ATTRS, FAN_ATTRS, FAN_ATTRS), NULL,
};
static const struct hwmon_chip_info fan_chip = { .ops = &fan_ops, .info = fan_info };

static int fan_probe(struct i2c_client *client)
{
	struct as7760_fan *fan;
	struct device *hwmon;
	int duty;
	if (!i2c_check_functionality(client->adapter, I2C_FUNC_SMBUS_BYTE_DATA))
		return -EOPNOTSUPP;
	duty = i2c_smbus_read_byte_data(client, 0x11);
	if (duty < 0) return duty;
	if (duty > 15) return -ENODEV;
	fan = devm_kzalloc(&client->dev, sizeof(*fan), GFP_KERNEL);
	if (!fan) return -ENOMEM;
	fan->client = client;
	mutex_init(&fan->lock);
	hwmon = devm_hwmon_device_register_with_info(&client->dev, "as7760_fan",
		fan, &fan_chip, NULL);
	return PTR_ERR_OR_ZERO(hwmon);
}

static const struct of_device_id fan_of_match[] = {
	{ .compatible = "accton,as7760-fan" }, { }
};
MODULE_DEVICE_TABLE(of, fan_of_match);
static struct i2c_driver fan_driver = {
	.driver = { .name = "as7760_fan", .of_match_table = fan_of_match },
	.probe = fan_probe,
};
module_i2c_driver(fan_driver);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Edge-Core AS7760 six dual-rotor fans and shared PWM");
