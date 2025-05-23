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
POST http://queue/queue/add_task
>>>
{ "submission_id": "arbitrary_id" }


Worker -> Queue
GET http://queue/queue/get_task
<<<
{
    "id": "hex_generated_task_id",
    "submission_id": "arbitrary_id",
    "exploit": "exploit code or arbitraty data"
}
// or
null // when queue is empty

POST http://queue/queue/submit_completed
>>>
{
    "id": "hex_generated_task_id",
    "info": "arbitrary data"
}


Queue -> Exploit storage
GET http://exploit_storage/get_exploit/{submission_id}
<<<
plain-text: exploit code or arbitraty data


Queue -> Collector
POST http://collector/submit
>>>
{
    "submission_id": "arbitrary_id",
    "info": "arbitrary data"
}
```

Очередь синхронизируется с диском, при падении и перезапуске очередь будет восстановлена

В очередь встроен кеш, который кеширует in-memory данные с Exploit storage

`queue/get_task` реализован с long polling, при пустой очереди ответ `null` придёт только через таймаут, при появлении задачи ответ придёт сразу

После `queue/get_task` должен следовать `queue/submit_completed` до заданного таймаута, иначе задача будет отдана другому воркеру

Что угодно можно изменить по желанию
