<?php
use App\Http\Controllers\JobController;

Route::post('/jobs', [JobController::class, 'create']);
Route::post('/shadow', [JobController::class, 'shadowed']);
Route::post('/other', [JobController::class, 'otherFile']);
Route::post('/text', [JobController::class, 'textOnly']);
