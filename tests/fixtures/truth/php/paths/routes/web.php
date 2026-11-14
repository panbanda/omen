<?php
use App\Http\Controllers\RunController;

Route::get('/run', [RunController::class, 'run']);
