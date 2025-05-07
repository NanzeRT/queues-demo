# Queue demo

Пример использования очереди и кеша

В примере 5 сервисов:
1. Queue with cache (main.rs)
2. Worker (worker.rs)
3. Client (client.rs)
4. Collector (collector.rs)
5. Exploit storage (exploit_storage.rs)

```
Client -> Queue
http://queue/queue/add_task
{ "submission_id": "arbitrary_id" }


Worker -> Queue
http://queue/queue/get_task
{
    "id": "hex_generated_task_id",
    "submission_id": "arbitrary_id",
    "exploit": "exploit code or arbitraty data"
}
// or
null // when queue is empty

http://queue/queue/submit_completed
{
    "id": "hex_generated_task_id",
    "info": "arbitrary data"
}


Queue -> Exploit storage
http://exploit_storage/get_exploit/{submission_id}
plain-text: exploit code or arbitraty data


Queue -> Collector
http://collector/submit
{
    "submission_id": "arbitrary_id",
    "info": "arbitrary data"
}
```

В очередь встроен кеш, который кеширует in-memory данные с Exploit storage

`queue/get_task` реализован с long polling, при пустой очереди ответ `null` придёт только через таймаут, при появлении задачи ответ придёт сразу

Что угодно можно изменить по желанию
