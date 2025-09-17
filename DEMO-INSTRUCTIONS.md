Follow these steps to prepare your setup for the demo:

1. The first change involves how you notify the `notifications-service`.

   When performing a request to the `notifications-service` you should add a
   new query parameter that is used to identify your group. For this purpose, a
   new file named `token` will be added to your group's credentials
   folder. The contents of this file, is the value of the `token` query
   parameter you have to pass when notifying the `notifications-service`.

   Additionally, you also have to change the host to which you are sending the
   request to `notifications-service-cec.ad.dlandau.nl`.

   As such, when performing a request to the notifications-service, the url
   should look as follows:
   `https://notifications-service-cec.ad.dlandau.nl/api/notify?token=<your-token>`.

   **You can keep sending the body as you were.**
1. Your REST API should be reachable at `<your-vm-ip>:3003`. 

   This implies that when querying your REST API, the endpoints will be: 
   - Temperature:
     `http://<your-vm-ip>:3003/temperature?experiment-id=<experiment-id>&start-time=<start-time>&end-time=<end-time>`
   - Out-of-bounds:
     `http://<your-vm-ip>:3003/temperature/out-of-range?experiment-id=<experiment-id>`
1. A grafana instance is provided so you can view the current perceived state
   of your infrastructure.

   Visit `https://grafana-cec.ad.dlandau.nl/` and login with the credentials
   provided in the `grafana` file in your group's credentials folder in one
   drive.
1. Have your consumers read from the `experiment` topic instead of your group
   topic. The data produced throughout the demo will go to this topic.

   > Note: Your group client only has read permissions on this topic, therefore
   > you cannot run your producers to insert data into the topic. 

# Optional


1. To display CPU and Memory metrics from your VM, you can expose a prometheus
   service that scrapes metrics from a `node_exporter`. The prometheus
   instance should be reachable at `<your-vm-ip>:3008`. 
   > Note: you only need the `prometheus` service from the [local setup](https://github.com/EC-labs/cec-tutorials/tree/master/prometheus#local-setup)
   > `docker-compose` file running, i.e., you don't need to have the grafana
   > service running.

