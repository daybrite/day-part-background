package dev.daybrite.background;

import android.content.Context;
import androidx.annotation.NonNull;
import androidx.work.Worker;
import androidx.work.WorkerParameters;
import dev.daybrite.day.bridge.DayRuntime;
import dev.daybrite.day.bridge.day_part_background.DayPartBackgroundBridge;

/** WorkManager owns persistence and process restart; the Rust registry owns task handlers. */
public final class DayBackgroundWorker extends Worker {
    private volatile long run;
    public DayBackgroundWorker(@NonNull Context context, @NonNull WorkerParameters parameters) {
        super(context, parameters);
    }
    @NonNull @Override public Result doWork() {
        try {
            DayRuntime.startServices(getApplicationContext());
            synchronized (this) {
                if (isStopped()) return Result.retry();
                run = DayPartBackgroundBridge.nextRun();
            }
            String result = DayPartBackgroundBridge.executeWorker(
                getInputData().getString("identifier"), getInputData().getString("payload"), run);
            if (isStopped()) return Result.retry();
            return "success".equals(result) ? Result.success() : "retry".equals(result) ? Result.retry() : Result.failure();
        } catch (Exception e) {
            android.util.Log.e("Day", "Background worker failed", e);
            return Result.retry();
        } finally { DayPartBackgroundBridge.releaseWorker(run); }
    }
    @Override public synchronized void onStopped() { DayPartBackgroundBridge.cancelWorker(run); }
}
