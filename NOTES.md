## Pinging NXP S32K148EVB-Q176 over 100/1000 BASE-T1 MediaConverter

Get the adapter name with commands:
```
ip -br link
ip -br addr
```

It will look like `enxa0cec8abd6ad`, plug it to the command below:

```
nmcli con add type ethernet ifname enxa0cec8abd6ad con-name nxp \
  ipv4.method manual ipv4.addresses 192.168.0.10/24 ipv6.method disabled
nmcli con up nxp
```

Ping it with specifying the network adapter:

```
ping -I enxa0cec8abd6ad 192.168.0.200
```
