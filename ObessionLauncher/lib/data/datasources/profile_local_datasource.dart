import 'dart:convert';
import 'dart:io' as io;
import 'dart:math';

import '../../domain/entities/profile.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';
import '../models/profile_model.dart';

/// Локальное хранилище профилей.
class ProfileLocalDataSource {
  ProfileLocalDataSource({
    required this._paths,
    required this._log,
  });

  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  static const _activeProfileFile = 'active_profile.json';
  static const _profilesFile = 'profiles.json';

  String get _activePath => _paths.profilePath(_activeProfileFile);
  String get _profilesPath => _paths.profilePath(_profilesFile);

  Future<List<Profile>> loadProfiles() async {
    final file = io.File(_profilesPath);
    if (!file.existsSync()) return [];
    try {
      final raw = await file.readAsString();
      final list = jsonDecode(raw) as List<dynamic>;
      return list.map((e) => ProfileModel.fromJson(e as Map<String, dynamic>).toEntity()).toList();
    } catch (e) {
      _log.error('Ошибка загрузки профилей: $e');
      return [];
    }
  }

  Future<Profile?> loadActiveProfile() async {
    final file = io.File(_activePath);
    if (!file.existsSync()) return null;
    try {
      final raw = await file.readAsString();
      final json = jsonDecode(raw) as Map<String, dynamic>;
      return ProfileModel.fromJson(json).toEntity();
    } catch (e) {
      _log.error('Ошибка загрузки активного профиля: $e');
      return null;
    }
  }

  Future<void> saveProfiles(List<Profile> profiles) async {
    final file = io.File(_profilesPath);
    final models = profiles.map(ProfileModel.fromEntity).toList();
    await file.writeAsString(jsonEncode(models.map((m) => m.toJson()).toList()));
  }

  Future<void> saveActiveProfile(Profile profile) async {
    final file = io.File(_activePath);
    final model = ProfileModel.fromEntity(profile);
    await file.writeAsString(jsonEncode(model.toJson()));
  }

  Future<void> deleteProfile(String id) async {
    final profiles = await loadProfiles();
    final filtered = profiles.where((p) => p.id != id).toList();
    await saveProfiles(filtered);
  }

  Profile createProfile(String name) {
    final id = _generateId();
    return Profile.create(id: id, name: name);
  }

  String _generateId() {
    final rng = Random.secure();
    final bytes = List<int>.generate(8, (_) => rng.nextInt(256));
    return bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  }
}
