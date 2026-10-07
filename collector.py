import csv
import json
import time
from datetime import datetime, timezone

import zenoh


# ---------------------------------------------------------
# Configuration
# ---------------------------------------------------------

ZENOH_ENDPOINT = "tcp/localhost:7447"

# Subscribe to all uProtocol events from guardian-vss
SUBSCRIPTION_KEY = "up/guardian-vss/**"

CSV_FILE = "realtime_data.csv"


# ---------------------------------------------------------
# CSV columns
# ---------------------------------------------------------

CSV_COLUMNS = [
    "receive_timestamp",
    "event_timestamp_ms",
    "event_type",

    "child_present",
    "child_confidence",
    "child_zone",

    "temperature_celsius",
    "temperature_sensor_status",

    "guardian_state",
    "guardian_child_present",
    "guardian_temperature_celsius",

    "window_percentage",
    "window_alarm_enabled",

    "hvac_target_temperature_celsius",
    "hvac_active",
    "hvac_fan_speed_percent",
    "hvac_fault_active",
]


# ---------------------------------------------------------
# Resource IDs
# ---------------------------------------------------------

EVENT_TYPES = {
    "9001": "child_presence",
    "9002": "temperature",
    "9003": "guardian_state",
    "9004": "window_state",
    "9007": "hvac_state",
}


# ---------------------------------------------------------
# CSV initialization
# ---------------------------------------------------------

def initialize_csv():
    with open(CSV_FILE, "w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=CSV_COLUMNS)
        writer.writeheader()


# ---------------------------------------------------------
# Extract resource ID from Zenoh key
# ---------------------------------------------------------

def get_resource_id(key):
    """
    Example key:

    up/guardian-vss/9000/0/1/9002/{}/{}/{}/{}/{}

    The resource ID is 9002.
    """

    parts = str(key).split("/")

    try:
        # parts:
        # 0 = up
        # 1 = guardian-vss
        # 2 = 9000
        # 3 = 0
        # 4 = 1
        # 5 = resource ID

        return parts[5]

    except (IndexError, AttributeError):
        return None


# ---------------------------------------------------------
# Process incoming event
# ---------------------------------------------------------

def process_event(sample):

    key = str(sample.key_expr)

    resource_id = get_resource_id(key)

    if resource_id not in EVENT_TYPES:
        return

    event_type = EVENT_TYPES[resource_id]

    try:
        payload = bytes(sample.payload)
        data = json.loads(payload.decode("utf-8"))

    except Exception as error:
        print("Could not decode event:")
        print(error)
        print("Key:", key)
        print("Payload:", sample.payload)
        return

    receive_timestamp = datetime.now(timezone.utc).isoformat()

    row = {
        "receive_timestamp": receive_timestamp,
        "event_timestamp_ms": data.get("timestamp_ms"),
        "event_type": event_type,

        "child_present": None,
        "child_confidence": None,
        "child_zone": None,

        "temperature_celsius": None,
        "temperature_sensor_status": None,

        "guardian_state": None,
        "guardian_child_present": None,
        "guardian_temperature_celsius": None,

        "window_percentage": None,
        "window_alarm_enabled": None,

        "hvac_target_temperature_celsius": None,
        "hvac_active": None,
        "hvac_fan_speed_percent": None,
        "hvac_fault_active": None,
    }

    # -----------------------------------------------------
    # Child presence
    # -----------------------------------------------------

    if event_type == "child_presence":

        row["child_present"] = data.get("present")
        row["child_confidence"] = data.get("confidence")
        row["child_zone"] = data.get("zone")


    # -----------------------------------------------------
    # Temperature
    # -----------------------------------------------------

    elif event_type == "temperature":

        row["temperature_celsius"] = data.get(
            "temperature_celsius"
        )

        row["temperature_sensor_status"] = data.get(
            "sensor_status"
        )


    # -----------------------------------------------------
    # Guardian
    # -----------------------------------------------------

    elif event_type == "guardian_state":

        row["guardian_state"] = data.get("state")

        row["guardian_child_present"] = data.get(
            "child_present"
        )

        row["guardian_temperature_celsius"] = data.get(
            "temperature_celsius"
        )


    # -----------------------------------------------------
    # Window
    # -----------------------------------------------------

    elif event_type == "window_state":

        row["window_percentage"] = data.get(
            "window_percentage"
        )

        row["window_alarm_enabled"] = data.get(
            "alarm_enabled"
        )


    # -----------------------------------------------------
    # HVAC
    # -----------------------------------------------------

    elif event_type == "hvac_state":

        row["hvac_target_temperature_celsius"] = data.get(
            "target_temperature_celsius"
        )

        row["hvac_active"] = data.get(
            "air_conditioning_active"
        )

        row["hvac_fan_speed_percent"] = data.get(
            "fan_speed_percent"
        )

        row["hvac_fault_active"] = data.get(
            "fault_active"
        )


    # -----------------------------------------------------
    # Write row
    # -----------------------------------------------------

    with open(
        CSV_FILE,
        "a",
        newline="",
        encoding="utf-8"
    ) as file:

        writer = csv.DictWriter(
            file,
            fieldnames=CSV_COLUMNS
        )

        writer.writerow(row)


    # -----------------------------------------------------
    # Display received event
    # -----------------------------------------------------

    print(
        f"[{receive_timestamp}] "
        f"{event_type}: "
        f"{data}"
    )


# ---------------------------------------------------------
# Main
# ---------------------------------------------------------

def main():

    print("==============================================")
    print(" Real-Time uProtocol / Zenoh Data Collector")
    print("==============================================")

    print()
    print("Connecting to Zenoh...")
    print(f"Endpoint: {ZENOH_ENDPOINT}")
    print(f"Subscription: {SUBSCRIPTION_KEY}")
    print()

    initialize_csv()

    config = zenoh.Config()

    config.insert_json5(
        "mode",
        '"client"'
    )

    config.insert_json5(
        "connect/endpoints",
        f'["{ZENOH_ENDPOINT}"]'
    )

    with zenoh.open(config) as session:

        print("Connected to Zenoh.")
        print()

        session.declare_subscriber(
            SUBSCRIPTION_KEY,
            process_event
        )

        print("Subscribed successfully.")
        print(f"Writing data to: {CSV_FILE}")
        print()
        print("Waiting for real-time events...")
        print("Press Ctrl+C to stop.")
        print()

        try:

            while True:
                time.sleep(1)

        except KeyboardInterrupt:

            print()
            print("Collector stopped.")


if __name__ == "__main__":
    main()