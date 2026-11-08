import android.content.Context;
import androidx.work.*;
import dev.daybrite.day.bridge.DayRuntime;
import dev.daybrite.background.DayBackgroundWorker;
import org.json.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicLong;

private static volatile long eventToken;
private static final AtomicLong counter = new AtomicLong();
private static final ConcurrentHashMap<Long, Execution> running = new ConcurrentHashMap<>();
private static final Set<String> identifiers = ConcurrentHashMap.newKeySet();
private static class Execution {
    final CountDownLatch done = new CountDownLatch(1);
    volatile String result = "retry";
    boolean cancelled;
}
public static void events_native(long token) { eventToken = token; }
public static void initialize_native(String text) throws JSONException {
    JSONArray array = new JSONArray(text);
    for (int i = 0; i < array.length(); i++) identifiers.add(array.getString(i));
}
private static void emit(JSONObject value) { events_native_emit(eventToken, value.toString()); }
public static long nextRun() {
    long run = counter.incrementAndGet();
    running.put(run, new Execution());
    return run;
}
public static String executeWorker(String identifier, String payload, long run) throws Exception {
    if (eventToken == 0 || !identifiers.contains(identifier)) return "failed";
    Execution execution = running.get(run);
    if (execution == null) return "retry";
    try {
        synchronized (execution) {
            if (execution.cancelled) return "retry";
            emit(new JSONObject().put("event", "run").put("run", run).put("identifier", identifier).put("payload", payload));
        }
        if (!execution.done.await(8, TimeUnit.MINUTES)) {
            cancelWorker(run); return "retry";
        }
        return execution.result;
    } finally { running.remove(run); }
}
public static void cancelWorker(long run) {
    Execution execution = running.get(run);
    if (execution == null) return;
    synchronized (execution) {
        execution.cancelled = true;
        execution.result = "retry";
        try { emit(new JSONObject().put("event", "cancel").put("run", run)); }
        catch (JSONException ignored) {}
        execution.done.countDown();
    }
}
public static void releaseWorker(long run) {
    cancelWorker(run);
    running.remove(run);
}
public static void finish_native(long run, String result) {
    Execution execution = running.get(run);
    if (execution != null) synchronized (execution) {
        if (!execution.cancelled) execution.result = result;
        execution.done.countDown();
    }
}
public static void schedule_native(String text, long done) {
    try {
        JSONObject request = new JSONObject(text);
        String id = request.getString("identifier");
        if (!identifiers.contains(id)) throw new IllegalArgumentException("Task is not registered");
        Data data = new Data.Builder().putString("identifier", id).putString("payload", request.getString("payload")).build();
        Constraints constraints = new Constraints.Builder()
            .setRequiredNetworkType(request.getBoolean("requires_network") ? NetworkType.CONNECTED : NetworkType.NOT_REQUIRED)
            .setRequiresCharging(request.getBoolean("requires_charging")).build();
        long delay = request.getLong("delay_secs");
        WorkManager manager = WorkManager.getInstance(DayRuntime.ctx.getApplicationContext());
        Operation operation;
        if (!request.isNull("interval_secs")) {
            PeriodicWorkRequest task = new PeriodicWorkRequest.Builder(DayBackgroundWorker.class, request.getLong("interval_secs"), TimeUnit.SECONDS)
                .setInputData(data).setConstraints(constraints).setInitialDelay(delay, TimeUnit.SECONDS)
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build();
            operation = manager.enqueueUniquePeriodicWork(id, ExistingPeriodicWorkPolicy.CANCEL_AND_REENQUEUE, task);
        } else {
            OneTimeWorkRequest task = new OneTimeWorkRequest.Builder(DayBackgroundWorker.class)
                .setInputData(data).setConstraints(constraints).setInitialDelay(delay, TimeUnit.SECONDS)
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build();
            operation = manager.enqueueUniqueWork(id, ExistingWorkPolicy.REPLACE, task);
        }
        operation.getResult().addListener(() -> {
            try { operation.getResult().get(); schedule_native_complete(done, ""); }
            catch (Exception e) { schedule_native_complete(done, e.toString()); }
        }, Runnable::run);
    } catch (Exception e) { schedule_native_complete(done, e.toString()); }
}
public static void cancel_native(String identifier, long done) {
    Operation operation = WorkManager.getInstance(DayRuntime.ctx.getApplicationContext()).cancelUniqueWork(identifier);
    operation.getResult().addListener(() -> {
        try { operation.getResult().get(); cancel_native_complete(done, ""); }
        catch (Exception e) { cancel_native_complete(done, e.toString()); }
    }, Runnable::run);
}
