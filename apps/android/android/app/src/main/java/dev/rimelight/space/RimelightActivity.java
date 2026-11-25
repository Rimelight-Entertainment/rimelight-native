package dev.rimelight.space;

import android.app.NativeActivity;
import android.content.Intent;

/**
 * The NativeActivity of the app, with one override. android-activity does not forward a new intent to Rust, and a
 * singleTask activity gets the sign-in return (rimelight-space://auth) as a new intent. Keeping that intent as the
 * activity's intent lets the Rust side read it with getIntent() (rimelight_platform, PLAN.md section 15.11).
 */
public class RimelightActivity extends NativeActivity {
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
    }
}
