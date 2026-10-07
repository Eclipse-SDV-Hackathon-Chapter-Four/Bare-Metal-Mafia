# ============================================================
# REAL-TIME VEHICLE CABIN SAFETY ANALYSIS
# ============================================================
#
# Main anomaly categories:
#   1. HVAC anomalies
#   2. Temperature anomalies
#   3. Child-safety anomalies
#
# Separate ML task:
#   Predict cabin temperature behavior 10 minutes ahead
#   Classes: NORMAL / SPIKE / DROP
#
# Final outputs:
#
# results/
# ├── anomaly_detection_results.csv
# ├── temperature_prediction_dataset.csv
# ├── model_results.csv
# └── visualizations/
#     ├── 01_event_distribution.png
#     ├── 02_prediction_target_distribution.png
#     ├── 03_anomaly_category_summary.png
#     ├── 04_correlation_heatmap.png
#     ├── 05_operating_state_clusters.png
#     ├── 06_temperature_anomaly_timeline.png
#     ├── 07_hvac_anomaly_timeline.png
#     ├── 08_child_safety_timeline.png
#     ├── 09_anomaly_overlap.png
#     ├── 10_logistic_confusion_matrix.png
#     └── 11_logistic_feature_importance.png
#
# ============================================================


import os
import warnings

import numpy as np
import pandas as pd
import matplotlib.pyplot as plt

from sklearn.preprocessing import StandardScaler
from sklearn.linear_model import LogisticRegression

from sklearn.preprocessing import LabelEncoder
from xgboost import XGBClassifier
from sklearn.model_selection import train_test_split
from sklearn.metrics import (
    classification_report,
    confusion_matrix,
    accuracy_score,
    balanced_accuracy_score,
    f1_score
)
from sklearn.cluster import KMeans
from sklearn.decomposition import PCA


warnings.filterwarnings("ignore")


# ============================================================
# 1. CONFIGURATION
# ============================================================

INPUT_FILE = "realtime_data_L.csv"

OUTPUT_DIR = "results"
VIS_DIR = os.path.join(OUTPUT_DIR, "visualizations")

os.makedirs(OUTPUT_DIR, exist_ok=True)
os.makedirs(VIS_DIR, exist_ok=True)


# ------------------------------------------------------------
# Thresholds
# ------------------------------------------------------------

TEMPERATURE_SPIKE_THRESHOLD = 2.0
TEMPERATURE_DROP_THRESHOLD = 2.0

CHILD_WARNING_TEMP = 26.0
CHILD_HIGH_TEMP = 28.0
CHILD_CRITICAL_TEMP = 30.0

HVAC_TOLERANCE = 2.0

PREDICTION_HORIZON_MINUTES = 10


# ============================================================
# 2. REQUIRED SCHEMA
# ============================================================

REQUIRED_COLUMNS = [
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
    "hvac_fault_active"
]


# ============================================================
# 3. LOAD DATA
# ============================================================

print("=" * 70)
print("LOADING DATA")
print("=" * 70)

df = pd.read_csv(INPUT_FILE)

print(f"Input shape: {df.shape}")


missing_columns = [
    col for col in REQUIRED_COLUMNS
    if col not in df.columns
]

if missing_columns:
    raise ValueError(
        f"Missing required columns: {missing_columns}"
    )

df = df[REQUIRED_COLUMNS].copy()

print("Schema validation: PASSED")


# ============================================================
# 4. TIMESTAMP PROCESSING
# ============================================================

df["receive_timestamp"] = pd.to_datetime(
    df["receive_timestamp"],
    errors="coerce",
    utc=True
)



# ------------------------------------------------------------
# Final timestamp validation
# ------------------------------------------------------------

df["receive_timestamp"] = pd.to_datetime(
    df["receive_timestamp"],
    errors="coerce",
    utc=True
)

if df["receive_timestamp"].isna().any():

    raise ValueError(
        "receive_timestamp still contains null values."
    )


# Sort chronologically
df = (
    df.sort_values("receive_timestamp")
      .reset_index(drop=True)
)


# ------------------------------------------------------------
# Event timestamp
# ------------------------------------------------------------

if "event_timestamp_ms" in df.columns:

    df["event_timestamp"] = pd.to_datetime(
        df["event_timestamp_ms"],
        unit="ms",
        errors="coerce",
        utc=True
    )

else:

    df["event_timestamp"] = df["receive_timestamp"]



# ------------------------------------------------------------
# Event timestamp
# ------------------------------------------------------------

if "event_timestamp_ms" in df.columns:

    df["event_timestamp"] = pd.to_datetime(
        df["event_timestamp_ms"],
        unit="ms",
        errors="coerce",
        utc=True
    )

else:

    df["event_timestamp"] = df["receive_timestamp"]




# ============================================================
# 5. DATA TYPE CONVERSION
# ============================================================

BOOLEAN_COLUMNS = [
    "child_present",
    "window_alarm_enabled",
    "hvac_active",
    "hvac_fault_active",
    "guardian_child_present"
]

NUMERIC_COLUMNS = [
    "child_confidence",
    "temperature_celsius",
    "guardian_temperature_celsius",
    "window_percentage",
    "hvac_target_temperature_celsius",
    "hvac_fan_speed_percent"
]


for col in BOOLEAN_COLUMNS:

    if col in df.columns:

        df[col] = (
            df[col]
            .astype(str)
            .str.strip()
            .str.lower()
            .map({
                "true": True,
                "false": False,
                "1": True,
                "0": False
            })
        )


for col in NUMERIC_COLUMNS:

    df[col] = pd.to_numeric(
        df[col],
        errors="coerce"
    )


# ============================================================
# 6. FORWARD-FILL SYSTEM STATE
# ============================================================
#
# The raw data is event-based.
#
# Therefore:
# temperature event -> temperature available
# guardian event   -> guardian state available
# child event      -> child state available
# HVAC event       -> HVAC state available
# window event     -> window state available
#
# Forward filling reconstructs the latest known system state.
# ============================================================

STATE_COLUMNS = [
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
    "hvac_fault_active"
]

df[STATE_COLUMNS] = df[STATE_COLUMNS].ffill()


# ============================================================
# 7. BASIC DATASET INFORMATION
# ============================================================

print("\n" + "=" * 70)
print("DATASET OVERVIEW")
print("=" * 70)

print(f"Rows: {len(df)}")
print(f"Columns: {len(df.columns)}")

print("\nEvent distribution:")
print(df["event_type"].value_counts())


# ============================================================
# 8. VISUALIZATION 1
#    EVENT DISTRIBUTION
# ============================================================

plt.figure(figsize=(9, 5))

df["event_type"].value_counts().plot(
    kind="bar"
)

plt.title("Event Type Distribution")
plt.xlabel("Event Type")
plt.ylabel("Number of Events")
plt.xticks(rotation=45)
plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "01_event_distribution.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 9. FEATURE ENGINEERING
# ============================================================

df["temperature_change"] = (
    df["temperature_celsius"]
    .diff()
)

df["guardian_temperature_change"] = (
    df["guardian_temperature_celsius"]
    .diff()
)

df["temperature_rolling_mean"] = (
    df["temperature_celsius"]
    .rolling(
        window=5,
        min_periods=1
    )
    .mean()
)

df["temperature_rolling_std"] = (
    df["temperature_celsius"]
    .rolling(
        window=5,
        min_periods=2
    )
    .std()
)

time_difference_minutes = (
    df["receive_timestamp"]
    .diff()
    .dt.total_seconds()
    / 60
)

df["temperature_rate_per_minute"] = (
    df["temperature_change"]
    / time_difference_minutes.replace(0, np.nan)
)

df["temperature_vs_target"] = (
    df["temperature_celsius"]
    - df["hvac_target_temperature_celsius"]
)

df["hvac_temperature_error"] = (
    df["temperature_celsius"]
    - df["hvac_target_temperature_celsius"]
)

df["window_open"] = (
    df["window_percentage"] >= 90
)


# ============================================================
# 10. FUTURE TEMPERATURE
# ============================================================
#
# Only actual temperature observations are used to determine
# the future temperature.
#
# The dataset is event-based, so many rows do not contain
# temperature values.
# ============================================================

temperature_events = df[
    [
        "receive_timestamp",
        "temperature_celsius"
    ]
].copy()


# Keep only rows that contain an actual temperature reading
temperature_events = temperature_events.dropna(
    subset=[
        "receive_timestamp",
        "temperature_celsius"
    ]
)


# Sort temperature observations chronologically
temperature_events = (
    temperature_events
    .sort_values("receive_timestamp")
    .reset_index(drop=True)
)


# Rename columns for the future-temperature lookup
temperature_events = temperature_events.rename(
    columns={
        "receive_timestamp": "future_timestamp",
        "temperature_celsius": "future_temperature"
    }
)


# Make sure the main dataframe has no null merge keys
df = df.dropna(
    subset=["receive_timestamp"]
).copy()

df = (
    df.sort_values("receive_timestamp")
      .reset_index(drop=True)
)


# Find the next temperature observation within 10 minutes
df = pd.merge_asof(
    df,
    temperature_events,
    left_on="receive_timestamp",
    right_on="future_timestamp",
    direction="forward",
    tolerance=pd.Timedelta(
        minutes=PREDICTION_HORIZON_MINUTES
    ),
    allow_exact_matches=False
)


# Calculate temperature change over the future horizon
df["future_temperature_change"] = (
    df["future_temperature"]
    - df["temperature_celsius"]
)


# ============================================================
# 11. PREDICTION TARGET
# ============================================================

df["temperature_event_next_10min"] = "NORMAL"

df.loc[
    df["future_temperature_change"]
    >= TEMPERATURE_SPIKE_THRESHOLD,
    "temperature_event_next_10min"
] = "SPIKE"

df.loc[
    df["future_temperature_change"]
    <= -TEMPERATURE_DROP_THRESHOLD,
    "temperature_event_next_10min"
] = "DROP"
# ============================================================
# 11A. 10-MINUTE FUTURE TEMPERATURE TARGET FOR ML
# ============================================================
#
# ML task:
# Predict whether cabin temperature will experience
# a SPIKE, DROP, or remain NORMAL within the next 10 minutes
# when a child is present.
#
# Only actual temperature events are used to calculate
# the future temperature behavior.
#
# Future information is used ONLY to create the target.
# It is NOT used as an ML feature.
# ============================================================


# ------------------------------------------------------------
# Actual temperature observations
# ------------------------------------------------------------

temperature_events_ml = df[
    [
        "receive_timestamp",
        "temperature_celsius"
    ]
].copy()


temperature_events_ml = (
    temperature_events_ml
    .dropna(
        subset=[
            "receive_timestamp",
            "temperature_celsius"
        ]
    )
    .sort_values("receive_timestamp")
    .reset_index(drop=True)
)


# ------------------------------------------------------------
# Create target columns
# ------------------------------------------------------------

df["future_max_temperature_10min"] = np.nan

df["future_min_temperature_10min"] = np.nan

df["future_max_change_10min"] = np.nan

df["future_min_change_10min"] = np.nan

df["temperature_event_next_10min_ml"] = pd.Series(
    pd.NA,
    index=df.index,
    dtype="object"
)


# ------------------------------------------------------------
# Calculate future temperature behavior
# ------------------------------------------------------------

for i in df.index:

    # Prediction is only made when a child is present
    if df.loc[i, "child_present"] is not True:
        continue

    # Current temperature must be available
    current_temperature = df.loc[
        i,
        "temperature_celsius"
    ]

    if pd.isna(current_temperature):
        continue

    current_timestamp = df.loc[
        i,
        "receive_timestamp"
    ]

    future_start = current_timestamp

    future_end = (
        current_timestamp
        + pd.Timedelta(
            minutes=PREDICTION_HORIZON_MINUTES
        )
    )


    # Actual temperature observations during
    # the following 10 minutes
    future_temperatures = temperature_events_ml.loc[
        (
            temperature_events_ml["receive_timestamp"]
            > future_start
        )
        &
        (
            temperature_events_ml["receive_timestamp"]
            <= future_end
        ),
        "temperature_celsius"
    ]


    if future_temperatures.empty:
        continue


    future_max = future_temperatures.max()

    future_min = future_temperatures.min()


    # Store future values
    df.loc[
        i,
        "future_max_temperature_10min"
    ] = future_max

    df.loc[
        i,
        "future_min_temperature_10min"
    ] = future_min


    # Calculate future changes
    max_change = (
        future_max
        - current_temperature
    )

    min_change = (
        current_temperature
        - future_min
    )


    df.loc[
        i,
        "future_max_change_10min"
    ] = max_change

    df.loc[
        i,
        "future_min_change_10min"
    ] = min_change


    # --------------------------------------------------------
    # Assign prediction target
    # --------------------------------------------------------

    if (
        max_change >= TEMPERATURE_SPIKE_THRESHOLD
        and
        min_change >= TEMPERATURE_DROP_THRESHOLD
    ):

        # Both occur within 10 minutes.
        # Select the stronger movement.

        if max_change >= min_change:

            df.loc[
                i,
                "temperature_event_next_10min_ml"
            ] = "SPIKE"

        else:

            df.loc[
                i,
                "temperature_event_next_10min_ml"
            ] = "DROP"


    elif max_change >= TEMPERATURE_SPIKE_THRESHOLD:

        df.loc[
            i,
            "temperature_event_next_10min_ml"
        ] = "SPIKE"


    elif min_change >= TEMPERATURE_DROP_THRESHOLD:

        df.loc[
            i,
            "temperature_event_next_10min_ml"
        ] = "DROP"


    else:

        df.loc[
            i,
            "temperature_event_next_10min_ml"
        ] = "NORMAL"


# ------------------------------------------------------------
# Target distribution
# ------------------------------------------------------------

print("\n" + "=" * 70)

print("10-MINUTE ML TARGET")

print("=" * 70)


ml_target = df[
    "temperature_event_next_10min_ml"
].dropna()


print(
    ml_target.value_counts()
)

# ============================================================
# 12. VISUALIZATION 2
#     PREDICTION TARGET DISTRIBUTION
# ============================================================

target_counts = (
    df[
        "temperature_event_next_10min_ml"
    ]
    .dropna()
    .value_counts()
)
plt.figure(figsize=(8, 5))

target_counts.plot(
    kind="bar"
)

plt.title(
    "10-Minute Child-Present Temperature Prediction Target"
)

plt.xlabel("Prediction Class")
plt.ylabel("Number of Records")
plt.xticks(rotation=0)
plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "02_prediction_target_distribution.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 13. HVAC ANOMALIES
# ============================================================

df["HVAC_STUCK_OFF"] = (
    (df["child_present"] == True)
    &
    (
        df["temperature_celsius"]
        >
        df["hvac_target_temperature_celsius"]
        + HVAC_TOLERANCE
    )
    &
    (df["hvac_active"] == False)
)

df["HVAC_INEFFECTIVE"] = (
    (df["child_present"] == True)
    &
    (df["hvac_active"] == True)
    &
    (
        df["temperature_change"] > 0
    )
    &
    (
        df["temperature_celsius"]
        >
        df["hvac_target_temperature_celsius"]
        + HVAC_TOLERANCE
    )
)

df["HVAC_FAULT"] = (
    df["hvac_fault_active"] == True
)


# ============================================================
# 14. TEMPERATURE ANOMALIES
# ============================================================

df["TEMPERATURE_SPIKE"] = (
    df["future_temperature_change"]
    >= TEMPERATURE_SPIKE_THRESHOLD
)

df["TEMPERATURE_DROP"] = (
    df["future_temperature_change"]
    <= -TEMPERATURE_DROP_THRESHOLD
)

df["SENSOR_ANOMALY"] = (
    df["temperature_sensor_status"]
    .astype(str)
    .str.upper()
    .eq("DEGRADED")
)


# ============================================================
# 15. CHILD-SAFETY ANOMALIES
# ============================================================

df["CHILD_TEMPERATURE_WARNING"] = (
    (df["child_present"] == True)
    &
    (df["temperature_celsius"] >= CHILD_WARNING_TEMP)
)

df["CHILD_TEMPERATURE_HIGH"] = (
    (df["child_present"] == True)
    &
    (df["temperature_celsius"] >= CHILD_HIGH_TEMP)
)

df["CHILD_TEMPERATURE_CRITICAL"] = (
    (df["child_present"] == True)
    &
    (df["temperature_celsius"] >= CHILD_CRITICAL_TEMP)
)

df["CHILD_HVAC_OFF_RISK"] = (
    (df["child_present"] == True)
    &
    (df["hvac_active"] == False)
    &
    (df["temperature_celsius"] >= CHILD_WARNING_TEMP)
)


# ============================================================
# 16. ANOMALY CATEGORY LABELS
# ============================================================

df["HVAC_ANOMALY"] = (
    df[
        [
            "HVAC_STUCK_OFF",
            "HVAC_INEFFECTIVE",
            "HVAC_FAULT"
        ]
    ]
    .any(axis=1)
)

df["TEMPERATURE_ANOMALY"] = (
    df[
        [
            "TEMPERATURE_SPIKE",
            "TEMPERATURE_DROP",
            "SENSOR_ANOMALY"
        ]
    ]
    .any(axis=1)
)

df["CHILD_SAFETY_ANOMALY"] = (
    df[
        [
            "CHILD_TEMPERATURE_WARNING",
            "CHILD_TEMPERATURE_HIGH",
            "CHILD_TEMPERATURE_CRITICAL",
            "CHILD_HVAC_OFF_RISK"
        ]
    ]
    .any(axis=1)
)


# ============================================================
# 17. ANOMALY SUMMARY
# ============================================================

anomaly_summary = pd.DataFrame({
    "Anomaly Category": [
        "HVAC",
        "Temperature",
        "Child Safety"
    ],
    "Records": [
        df["HVAC_ANOMALY"].sum(),
        df["TEMPERATURE_ANOMALY"].sum(),
        df["CHILD_SAFETY_ANOMALY"].sum()
    ]
})


print("\n" + "=" * 70)
print("ANOMALY CATEGORY SUMMARY")
print("=" * 70)

print(anomaly_summary.to_string(index=False))


# ============================================================
# 18. VISUALIZATION 3
#     ANOMALY CATEGORY SUMMARY
# ============================================================

plt.figure(figsize=(9, 5))

plt.bar(
    anomaly_summary["Anomaly Category"],
    anomaly_summary["Records"]
)

plt.title("Anomaly Category Summary")
plt.xlabel("Anomaly Category")
plt.ylabel("Number of Records")
plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "03_anomaly_category_summary.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 19. CORRELATION ANALYSIS
# ============================================================

correlation_features = [
    "temperature_celsius",
    "child_confidence",
    "guardian_temperature_celsius",
    "window_percentage",
    "hvac_target_temperature_celsius",
    "hvac_fan_speed_percent",
    "temperature_change",
    "temperature_rolling_mean",
    "temperature_rolling_std",
    "temperature_rate_per_minute",
    "temperature_vs_target",
    "hvac_temperature_error",
    "future_temperature_change"
]

correlation_data = df[
    correlation_features
].copy()

correlation_matrix = (
    correlation_data
    .corr()
)


# ============================================================
# 20. VISUALIZATION 4
#     CORRELATION HEATMAP
# ============================================================

plt.figure(figsize=(12, 10))

plt.imshow(
    correlation_matrix,
    aspect="auto"
)

plt.colorbar()

plt.xticks(
    range(len(correlation_matrix.columns)),
    correlation_matrix.columns,
    rotation=90
)

plt.yticks(
    range(len(correlation_matrix.columns)),
    correlation_matrix.columns
)

plt.title("Feature Correlation Matrix")

plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "04_correlation_heatmap.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 21. OPERATING STATE CLUSTERING
# ============================================================

cluster_features = [
    "temperature_celsius",
    "child_present",
    "hvac_active",
    "hvac_fan_speed_percent",
    "window_percentage"
]

cluster_data = df[
    cluster_features
].copy()

cluster_data = cluster_data.replace(
    [np.inf, -np.inf],
    np.nan
)

cluster_data = cluster_data.dropna()


if len(cluster_data) >= 3:

    scaler_cluster = StandardScaler()

    X_cluster = scaler_cluster.fit_transform(
        cluster_data
    )

    kmeans = KMeans(
        n_clusters=3,
        random_state=42,
        n_init=10
    )

    cluster_labels = kmeans.fit_predict(
        X_cluster
    )

    pca = PCA(
        n_components=2,
        random_state=42
    )

    X_pca = pca.fit_transform(
        X_cluster
    )

    # --------------------------------------------------------
    # Visualization 5
    # --------------------------------------------------------

    plt.figure(figsize=(9, 6))

    for cluster_id in sorted(
        np.unique(cluster_labels)
    ):

        mask = (
            cluster_labels == cluster_id
        )

        plt.scatter(
            X_pca[mask, 0],
            X_pca[mask, 1],
            label=f"Cluster {cluster_id}",
            alpha=0.6
        )

    plt.title(
        "Operating State Clusters"
    )

    plt.xlabel("Principal Component 1")
    plt.ylabel("Principal Component 2")

    plt.legend()
    plt.tight_layout()

    plt.savefig(
        os.path.join(
            VIS_DIR,
            "05_operating_state_clusters.png"
        ),
        dpi=300
    )

    plt.close()


# ============================================================
# 22. TEMPERATURE ANOMALY TIMELINE
# ============================================================

plt.figure(figsize=(14, 6))

plt.plot(
    df["receive_timestamp"],
    df["temperature_celsius"],
    label="Temperature"
)

spike_mask = (
    df["TEMPERATURE_SPIKE"]
)

drop_mask = (
    df["TEMPERATURE_DROP"]
)

sensor_mask = (
    df["SENSOR_ANOMALY"]
)

plt.scatter(
    df.loc[spike_mask, "receive_timestamp"],
    df.loc[spike_mask, "temperature_celsius"],
    label="Temperature Spike",
    s=25
)

plt.scatter(
    df.loc[drop_mask, "receive_timestamp"],
    df.loc[drop_mask, "temperature_celsius"],
    label="Temperature Drop",
    s=25
)

plt.scatter(
    df.loc[sensor_mask, "receive_timestamp"],
    df.loc[sensor_mask, "temperature_celsius"],
    label="Sensor Anomaly",
    s=35
)

plt.title(
    "Temperature Anomaly Timeline"
)

plt.xlabel("Time")
plt.ylabel("Temperature (°C)")

plt.legend()
plt.xticks(rotation=45)
plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "06_temperature_anomaly_timeline.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 23. HVAC ANOMALY TIMELINE
# ============================================================

plt.figure(figsize=(14, 6))

plt.plot(
    df["receive_timestamp"],
    df["temperature_celsius"],
    label="Temperature"
)

hvac_stuck = df["HVAC_STUCK_OFF"]

hvac_ineffective = df["HVAC_INEFFECTIVE"]

hvac_fault = df["HVAC_FAULT"]


plt.scatter(
    df.loc[hvac_stuck, "receive_timestamp"],
    df.loc[hvac_stuck, "temperature_celsius"],
    label="HVAC Stuck OFF",
    s=25
)

plt.scatter(
    df.loc[
        hvac_ineffective,
        "receive_timestamp"
    ],
    df.loc[
        hvac_ineffective,
        "temperature_celsius"
    ],
    label="HVAC Ineffective",
    s=25
)

plt.scatter(
    df.loc[
        hvac_fault,
        "receive_timestamp"
    ],
    df.loc[
        hvac_fault,
        "temperature_celsius"
    ],
    label="HVAC Fault",
    s=35
)

plt.title(
    "HVAC Anomaly Timeline"
)

plt.xlabel("Time")
plt.ylabel("Temperature (°C)")

plt.legend()
plt.xticks(rotation=45)
plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "07_hvac_anomaly_timeline.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 24. CHILD-SAFETY TIMELINE
# ============================================================

plt.figure(figsize=(14, 6))

child_mask = (
    df["child_present"] == True
)

plt.plot(
    df.loc[
        child_mask,
        "receive_timestamp"
    ],
    df.loc[
        child_mask,
        "temperature_celsius"
    ],
    label="Temperature while Child Present"
)

warning_mask = (
    child_mask
    &
    df["CHILD_TEMPERATURE_WARNING"]
)

high_mask = (
    child_mask
    &
    df["CHILD_TEMPERATURE_HIGH"]
)

critical_mask = (
    child_mask
    &
    df["CHILD_TEMPERATURE_CRITICAL"]
)

plt.scatter(
    df.loc[
        warning_mask,
        "receive_timestamp"
    ],
    df.loc[
        warning_mask,
        "temperature_celsius"
    ],
    label="Warning",
    s=25
)

plt.scatter(
    df.loc[
        high_mask,
        "receive_timestamp"
    ],
    df.loc[
        high_mask,
        "temperature_celsius"
    ],
    label="High",
    s=30
)

plt.scatter(
    df.loc[
        critical_mask,
        "receive_timestamp"
    ],
    df.loc[
        critical_mask,
        "temperature_celsius"
    ],
    label="Critical",
    s=35
)

plt.axhline(
    CHILD_WARNING_TEMP,
    linestyle="--",
    label="Warning Threshold"
)

plt.axhline(
    CHILD_HIGH_TEMP,
    linestyle="--",
    label="High Threshold"
)

plt.axhline(
    CHILD_CRITICAL_TEMP,
    linestyle="--",
    label="Critical Threshold"
)

plt.title(
    "Child-Safety Temperature Timeline"
)

plt.xlabel("Time")
plt.ylabel("Temperature (°C)")

plt.legend()
plt.xticks(rotation=45)
plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "08_child_safety_timeline.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 25. ANOMALY OVERLAP
# ============================================================

overlap_data = pd.DataFrame({
    "HVAC": df["HVAC_ANOMALY"],
    "Temperature": df["TEMPERATURE_ANOMALY"],
    "Child Safety": df["CHILD_SAFETY_ANOMALY"]
})

overlap_counts = {
    "HVAC only": (
        overlap_data["HVAC"]
        &
        ~overlap_data["Temperature"]
        &
        ~overlap_data["Child Safety"]
    ).sum(),

    "Temperature only": (
        overlap_data["Temperature"]
        &
        ~overlap_data["HVAC"]
        &
        ~overlap_data["Child Safety"]
    ).sum(),

    "Child Safety only": (
        overlap_data["Child Safety"]
        &
        ~overlap_data["HVAC"]
        &
        ~overlap_data["Temperature"]
    ).sum(),

    "HVAC + Temperature": (
        overlap_data["HVAC"]
        &
        overlap_data["Temperature"]
        &
        ~overlap_data["Child Safety"]
    ).sum(),

    "HVAC + Child Safety": (
        overlap_data["HVAC"]
        &
        overlap_data["Child Safety"]
        &
        ~overlap_data["Temperature"]
    ).sum(),

    "Temperature + Child Safety": (
        overlap_data["Temperature"]
        &
        overlap_data["Child Safety"]
        &
        ~overlap_data["HVAC"]
    ).sum(),

    "All Three": (
        overlap_data["HVAC"]
        &
        overlap_data["Temperature"]
        &
        overlap_data["Child Safety"]
    ).sum()
}


# ============================================================
# 26. VISUALIZATION 9
#     ANOMALY OVERLAP
# ============================================================

plt.figure(figsize=(11, 6))

plt.bar(
    list(overlap_counts.keys()),
    list(overlap_counts.values())
)

plt.title(
    "Overlap Between Anomaly Categories"
)

plt.xlabel("Anomaly Combination")
plt.ylabel("Number of Records")

plt.xticks(
    rotation=45,
    ha="right"
)

plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "09_anomaly_overlap.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 27. CHILD-SAFETY SEVERITY
# ============================================================

def get_child_safety_severity(row):

    if not row["child_present"]:
        return "NORMAL"

    temp = row["temperature_celsius"]

    if pd.isna(temp):
        return "NORMAL"

    if temp >= CHILD_CRITICAL_TEMP:
        return "CRITICAL"

    if temp >= CHILD_HIGH_TEMP:
        return "HIGH"

    if temp >= CHILD_WARNING_TEMP:
        return "WARNING"

    return "NORMAL"


df["child_safety_severity"] = (
    df.apply(
        get_child_safety_severity,
        axis=1
    )
)


# ============================================================
# 28. LOGISTIC REGRESSION DATASET
# ============================================================

ML_FEATURES = [
    "temperature_celsius",
    "child_confidence",
    "guardian_temperature_celsius",
    "window_percentage",
    "window_alarm_enabled",
    "hvac_target_temperature_celsius",
    "hvac_active",
    "hvac_fan_speed_percent",
    "hvac_fault_active",
    "temperature_change",
    "guardian_temperature_change",
    "temperature_vs_target",
    "temperature_rolling_mean",
    "temperature_rolling_std",
    "temperature_rate_per_minute",
    "hvac_temperature_error"
]


# ============================================================
# 28. MACHINE LEARNING DATASET
# ============================================================

ml_df = df.loc[
    (df["child_present"] == True)
    &
    (df["temperature_event_next_10min_ml"].notna()),
    ML_FEATURES
    +
    ["temperature_event_next_10min_ml"]
].copy()

# Convert boolean features

for col in [
    "window_alarm_enabled",
    "hvac_active",
    "hvac_fault_active"
]:

    ml_df[col] = (
        ml_df[col]
        .astype(float)
    )


# Remove invalid records

ml_df = ml_df.replace(
    [np.inf, -np.inf],
    np.nan
)

ml_df = ml_df.dropna()


X = ml_df[
    ML_FEATURES
].copy()

y = ml_df[
    "temperature_event_next_10min_ml"
].copy()


print("\n" + "=" * 70)
print("MACHINE LEARNING DATASET")
print("=" * 70)

print("X shape:", X.shape)
print("y shape:", y.shape)

print("\nTarget distribution:")
print(y.value_counts())


# ============================================================
# 29. SAVE FINAL ML DATASET
# ============================================================

ml_output = ml_df.copy()

ml_output.to_csv(
    os.path.join(
        OUTPUT_DIR,
        "temperature_prediction_dataset.csv"
    ),
    index=False
)


# ============================================================
# 30. TRAIN / TEST SPLIT
# ============================================================

X_train, X_test, y_train, y_test = train_test_split(
    X,
    y,
    test_size=0.20,
    random_state=42,
    stratify=y
)


# ============================================================
# 31. STANDARDIZATION
# ============================================================

scaler = StandardScaler()

X_train_scaled = scaler.fit_transform(
    X_train
)

X_test_scaled = scaler.transform(
    X_test
)


# ============================================================
# 32. LOGISTIC REGRESSION
# ============================================================

model = LogisticRegression(
    max_iter=1000,
    random_state=42
)

model.fit(
    X_train_scaled,
    y_train
)


# ============================================================
# 33. PREDICTIONS
# ============================================================

y_pred = model.predict(
    X_test_scaled
)


# ============================================================
# 34. MODEL METRICS
# ============================================================

accuracy = accuracy_score(
    y_test,
    y_pred
)

balanced_accuracy = balanced_accuracy_score(
    y_test,
    y_pred
)

macro_f1 = f1_score(
    y_test,
    y_pred,
    average="macro"
)

weighted_f1 = f1_score(
    y_test,
    y_pred,
    average="weighted"
)


print("\n" + "=" * 70)
print("LOGISTIC REGRESSION RESULTS")
print("=" * 70)

print(
    classification_report(
        y_test,
        y_pred
    )
)

print(
    f"Accuracy: {accuracy:.4f}"
)

print(
    f"Balanced Accuracy: {balanced_accuracy:.4f}"
)

print(
    f"Macro F1: {macro_f1:.4f}"
)

print(
    f"Weighted F1: {weighted_f1:.4f}"
)
# ============================================================
# 34A. XGBOOST
# ============================================================

print("\n" + "=" * 70)
print("XGBOOST")
print("=" * 70)


# ------------------------------------------------------------
# Encode target labels
# ------------------------------------------------------------

label_encoder = LabelEncoder()

y_train_xgb = label_encoder.fit_transform(
    y_train
)

y_test_xgb = label_encoder.transform(
    y_test
)

xgb_classes = label_encoder.classes_

print("XGBoost classes:", list(xgb_classes))


# ------------------------------------------------------------
# Handle class imbalance
# ------------------------------------------------------------

class_counts = pd.Series(
    y_train_xgb
).value_counts()

class_weights = {
    class_id:
    len(y_train_xgb)
    /
    (
        len(class_counts)
        * class_counts[class_id]
    )
    for class_id in class_counts.index
}

sample_weights = np.array([
    class_weights[class_id]
    for class_id in y_train_xgb
])


# ------------------------------------------------------------
# XGBoost model
# ------------------------------------------------------------

xgb_model = XGBClassifier(
    objective="multi:softprob",
    num_class=len(xgb_classes),

    n_estimators=300,
    max_depth=4,
    learning_rate=0.05,

    subsample=0.8,
    colsample_bytree=0.8,

    min_child_weight=2,

    reg_alpha=0.0,
    reg_lambda=1.0,

    eval_metric="mlogloss",

    random_state=42,
    n_jobs=-1,

    tree_method="hist"
)


# ------------------------------------------------------------
# Train
# ------------------------------------------------------------

xgb_model.fit(
    X_train,
    y_train_xgb,
    sample_weight=sample_weights
)


# ------------------------------------------------------------
# Predict
# ------------------------------------------------------------

y_pred_xgb_encoded = xgb_model.predict(
    X_test
)

y_pred_xgb = label_encoder.inverse_transform(
    y_pred_xgb_encoded
)


# ------------------------------------------------------------
# Metrics
# ------------------------------------------------------------

xgb_accuracy = accuracy_score(
    y_test,
    y_pred_xgb
)

xgb_balanced_accuracy = balanced_accuracy_score(
    y_test,
    y_pred_xgb
)

xgb_macro_f1 = f1_score(
    y_test,
    y_pred_xgb,
    average="macro"
)

xgb_weighted_f1 = f1_score(
    y_test,
    y_pred_xgb,
    average="weighted"
)


print("\nClassification Report:")
print(
    classification_report(
        y_test,
        y_pred_xgb,
        labels=["DROP", "NORMAL", "SPIKE"],
        zero_division=0
    )
)

print(
    f"Accuracy: {xgb_accuracy:.4f}"
)

print(
    f"Balanced Accuracy: "
    f"{xgb_balanced_accuracy:.4f}"
)

print(
    f"Macro F1: {xgb_macro_f1:.4f}"
)

print(
    f"Weighted F1: {xgb_weighted_f1:.4f}"
)

# ============================================================
# 35. CONFUSION MATRIX
# ============================================================

labels = [
    "DROP",
    "NORMAL",
    "SPIKE"
]

cm = confusion_matrix(
    y_test,
    y_pred,
    labels=labels
)


# ============================================================
# 36. VISUALIZATION 10
#     LOGISTIC REGRESSION CONFUSION MATRIX
# ============================================================

plt.figure(figsize=(7, 6))

plt.imshow(
    cm,
    aspect="auto"
)

plt.colorbar()

plt.xticks(
    range(len(labels)),
    labels
)

plt.yticks(
    range(len(labels)),
    labels
)

plt.xlabel("Predicted")
plt.ylabel("Actual")

plt.title(
    "Logistic Regression Confusion Matrix"
)


for i in range(
    len(labels)
):

    for j in range(
        len(labels)
    ):

        plt.text(
            j,
            i,
            cm[i, j],
            ha="center",
            va="center"
        )


plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "10_logistic_confusion_matrix.png"
    ),
    dpi=300
)

plt.close()

# ============================================================
# 36A. XGBOOST CONFUSION MATRIX
# ============================================================

labels = [
    "DROP",
    "NORMAL",
    "SPIKE"
]

cm_xgb = confusion_matrix(
    y_test,
    y_pred_xgb,
    labels=labels
)


plt.figure(figsize=(7, 6))

plt.imshow(
    cm_xgb,
    aspect="auto"
)

plt.colorbar()

plt.xticks(
    range(len(labels)),
    labels
)

plt.yticks(
    range(len(labels)),
    labels
)

plt.xlabel("Predicted")
plt.ylabel("Actual")

plt.title(
    "XGBoost Confusion Matrix"
)


for i in range(len(labels)):

    for j in range(len(labels)):

        plt.text(
            j,
            i,
            cm_xgb[i, j],
            ha="center",
            va="center"
        )


plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "12_xgboost_confusion_matrix.png"
    ),
    dpi=300
)

plt.close()
# ============================================================
# 37. LOGISTIC REGRESSION FEATURE IMPORTANCE
# ============================================================

coefficient_matrix = pd.DataFrame(
    model.coef_,
    columns=ML_FEATURES,
    index=model.classes_
)

mean_absolute_coefficients = (
    coefficient_matrix
    .abs()
    .mean(axis=0)
    .sort_values(
        ascending=False
    )
)

# ============================================================
# 37A. XGBOOST FEATURE IMPORTANCE
# ============================================================

xgb_importance = pd.Series(
    xgb_model.feature_importances_,
    index=ML_FEATURES
).sort_values(
    ascending=True
)


plt.figure(figsize=(10, 7))

xgb_importance.plot(
    kind="barh"
)

plt.title(
    "XGBoost Feature Importance"
)

plt.xlabel(
    "Importance"
)

plt.ylabel(
    "Feature"
)

plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "13_xgboost_feature_importance.png"
    ),
    dpi=300
)

plt.close()
# ============================================================
# 38. VISUALIZATION 11
#     LOGISTIC REGRESSION FEATURE IMPORTANCE
# ============================================================

plt.figure(figsize=(10, 7))

mean_absolute_coefficients.sort_values().plot(
    kind="barh"
)

plt.title(
    "Logistic Regression Feature Importance"
)

plt.xlabel(
    "Mean Absolute Coefficient"
)

plt.ylabel("Feature")

plt.tight_layout()

plt.savefig(
    os.path.join(
        VIS_DIR,
        "11_logistic_feature_importance.png"
    ),
    dpi=300
)

plt.close()


# ============================================================
# 39. FINAL ANOMALY RESULTS CSV
# ============================================================
#
# This contains the original event information plus the
# important anomaly results.
#
# It does NOT contain every intermediate analysis table.
# ============================================================

ANOMALY_OUTPUT_COLUMNS = [
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

    # HVAC category
    "HVAC_STUCK_OFF",
    "HVAC_INEFFECTIVE",
    "HVAC_FAULT",
    "HVAC_ANOMALY",

    # Temperature category
    "TEMPERATURE_SPIKE",
    "TEMPERATURE_DROP",
    "SENSOR_ANOMALY",
    "TEMPERATURE_ANOMALY",

    # Child-safety category
    "CHILD_TEMPERATURE_WARNING",
    "CHILD_TEMPERATURE_HIGH",
    "CHILD_TEMPERATURE_CRITICAL",
    "CHILD_HVAC_OFF_RISK",
    "CHILD_SAFETY_ANOMALY",

    "child_safety_severity"
]


anomaly_output = df[
    [
        col
        for col in ANOMALY_OUTPUT_COLUMNS
        if col in df.columns
    ]
].copy()


anomaly_output.to_csv(
    os.path.join(
        OUTPUT_DIR,
        "anomaly_detection_results.csv"
    ),
    index=False
)

# ============================================================
# 40. FINAL MODEL RESULTS CSV
# ============================================================

logistic_report = classification_report(
    y_test,
    y_pred,
    labels=["DROP", "NORMAL", "SPIKE"],
    output_dict=True,
    zero_division=0
)

xgb_report = classification_report(
    y_test,
    y_pred_xgb,
    labels=["DROP", "NORMAL", "SPIKE"],
    output_dict=True,
    zero_division=0
)


model_results = []


# ------------------------------------------------------------
# Logistic Regression results
# ------------------------------------------------------------

for class_name in [
    "DROP",
    "NORMAL",
    "SPIKE"
]:

    model_results.append({

        "model": "Logistic Regression",

        "result_type": "class_metrics",

        "class": class_name,

        "precision":
            logistic_report[class_name]["precision"],

        "recall":
            logistic_report[class_name]["recall"],

        "f1_score":
            logistic_report[class_name]["f1-score"],

        "support":
            logistic_report[class_name]["support"],

        "value": np.nan
    })


model_results.extend([

    {
        "model": "Logistic Regression",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": accuracy
    },

    {
        "model": "Logistic Regression",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": balanced_accuracy
    },

    {
        "model": "Logistic Regression",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": macro_f1
    },

    {
        "model": "Logistic Regression",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": weighted_f1
    }
])


# ------------------------------------------------------------
# XGBoost results
# ------------------------------------------------------------

for class_name in [
    "DROP",
    "NORMAL",
    "SPIKE"
]:

    model_results.append({

        "model": "XGBoost",

        "result_type": "class_metrics",

        "class": class_name,

        "precision":
            xgb_report[class_name]["precision"],

        "recall":
            xgb_report[class_name]["recall"],

        "f1_score":
            xgb_report[class_name]["f1-score"],

        "support":
            xgb_report[class_name]["support"],

        "value": np.nan
    })


model_results.extend([

    {
        "model": "XGBoost",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": xgb_accuracy
    },

    {
        "model": "XGBoost",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": xgb_balanced_accuracy
    },

    {
        "model": "XGBoost",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": xgb_macro_f1
    },

    {
        "model": "XGBoost",
        "result_type": "overall_metric",
        "class": "ALL",
        "precision": np.nan,
        "recall": np.nan,
        "f1_score": np.nan,
        "support": len(y_test),
        "value": xgb_weighted_f1
    }
])


model_results_df = pd.DataFrame(
    model_results
)

model_results_df.to_csv(
    os.path.join(
        OUTPUT_DIR,
        "model_results.csv"
    ),
    index=False
)

# ============================================================
# 41. FINAL SUMMARY
# ============================================================

print("\n" + "=" * 70)
print("ANOMALY COUNTS")
print("=" * 70)

print(
    "HVAC anomalies:",
    int(df["HVAC_ANOMALY"].sum())
)

print(
    "Temperature anomalies:",
    int(df["TEMPERATURE_ANOMALY"].sum())
)

print(
    "Child-safety anomalies:",
    int(df["CHILD_SAFETY_ANOMALY"].sum())
)


print("\n" + "=" * 70)
print("FINAL OUTPUT FILES")
print("=" * 70)

print(
    os.path.join(
        OUTPUT_DIR,
        "anomaly_detection_results.csv"
    )
)

print(
    os.path.join(
        OUTPUT_DIR,
        "temperature_prediction_dataset.csv"
    )
)

print(
    os.path.join(
        OUTPUT_DIR,
        "model_results.csv"
    )
)

print("\nVisualizations:")

for filename in sorted(
    os.listdir(VIS_DIR)
):

    if filename.endswith(".png"):
        print(
            os.path.join(
                VIS_DIR,
                filename
            )
        )


print("\n" + "=" * 70)
print("ANALYSIS COMPLETED")
print("=" * 70)